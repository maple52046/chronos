//! HTTP/HTTPS backend client that turns one `/time` request into a sample.
//!
//! The client enforces the transport security policy at construction (before
//! any request is issued) and validates TLS according to per-backend settings.

use std::error::Error as _;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use base64::prelude::{Engine as _, BASE64_STANDARD};
use chronos_core::estimate::estimate_offset_nanos;
use chronos_core::{BackendTransport, SampleQuality, SecurityPolicy, TimeSample, TimeStatus};
use reqwest::Url;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, RootCertStore, SignatureScheme};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::config::BackendConfig;

/// A configured client for a single time backend.
#[derive(Debug, Clone)]
pub struct BackendClient {
    name: String,
    url: Url,
    transport: BackendTransport,
    client: reqwest::Client,
}

/// Subset of the server `/time` response the gateway depends on.
///
/// Additional fields in the response are ignored, keeping the gateway tolerant
/// of forward-compatible server changes.
#[derive(Debug, Deserialize)]
struct BackendTimeResponse {
    server_send_unix_nano: i128,
    status: TimeStatus,
}

impl BackendClient {
    /// Builds a client for `config`, enforcing `policy` and TLS requirements.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL cannot be parsed, the backend requires TLS
    /// but is not `https`, the security policy forbids the transport, or the
    /// underlying HTTP client cannot be constructed.
    pub fn new(
        config: &BackendConfig,
        policy: SecurityPolicy,
        request_timeout: Duration,
        pinned_spki: &[String],
    ) -> anyhow::Result<Self> {
        let base_url = Url::parse(&config.base_url)
            .with_context(|| format!("parsing backend base URL {}", config.base_url))?;
        let url = time_endpoint_url(&base_url)
            .with_context(|| format!("building /time URL from base {}", config.base_url))?;
        let transport = classify_transport(&base_url);

        if config.require_tls && !matches!(transport, BackendTransport::Https) {
            anyhow::bail!(
                "backend {} sets require_tls but base URL {} is not https",
                config.name,
                config.base_url
            );
        }
        policy
            .evaluate(transport)
            .with_context(|| format!("backend {} rejected by security policy", config.name))?;

        // reqwest's `rustls-no-provider` requires an installed default provider.
        crate::ensure_crypto_provider();
        let mut builder = reqwest::Client::builder().timeout(request_timeout);
        if matches!(transport, BackendTransport::Https) && !pinned_spki.is_empty() {
            // Pinning subsumes standard verification: the custom verifier runs
            // full chain/hostname validation and then requires the leaf SPKI to
            // match a pin, so the lab-only invalid-cert escape hatch is ignored.
            let tls = build_pinning_tls(pinned_spki)
                .with_context(|| format!("building pinned TLS for backend {}", config.name))?;
            builder = builder.use_preconfigured_tls(tls);
        } else {
            // `require_valid_cert = false` is a lab-only escape hatch; disabling
            // verification accepts any presented certificate.
            builder = builder.danger_accept_invalid_certs(!config.require_valid_cert);
        }
        let client = builder.build().context("building HTTP client")?;

        Ok(Self {
            name: config.name.clone(),
            url,
            transport,
            client,
        })
    }

    /// Returns the backend's configured name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the backend's URL.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Returns the classified transport of this backend.
    #[must_use]
    pub fn transport(&self) -> BackendTransport {
        self.transport
    }

    /// Issues one `/time` request and produces a good [`TimeSample`].
    ///
    /// Round-trip time is measured with a monotonic [`Instant`]; the offset is
    /// estimated against the local wall clock captured at receive.
    ///
    /// # Errors
    ///
    /// Returns a [`SampleQuality`] describing why the sample is unusable: a
    /// failed request ([`SampleQuality::HttpError`] or
    /// [`SampleQuality::TlsError`]), an unparsable body
    /// ([`SampleQuality::InvalidResponse`]), or an unsynchronized backend
    /// ([`SampleQuality::BackendUnsynchronized`]).
    pub async fn fetch_sample(&self) -> Result<TimeSample, SampleQuality> {
        let started = Instant::now();
        let response = self
            .client
            .get(self.url.clone())
            .send()
            .await
            .map_err(|err| classify_request_error(&err))?;
        let response = response
            .error_for_status()
            .map_err(|_| SampleQuality::HttpError)?;
        let body = response
            .json::<BackendTimeResponse>()
            .await
            .map_err(|_| SampleQuality::InvalidResponse)?;
        let rtt_nanos = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let local_receive_unix_nanos = wall_now_nanos();

        if !body.status.is_synchronized() {
            return Err(SampleQuality::BackendUnsynchronized);
        }

        let estimated_offset_nanos = estimate_offset_nanos(
            body.server_send_unix_nano,
            local_receive_unix_nanos,
            rtt_nanos,
        );

        Ok(TimeSample {
            backend_name: self.name.clone(),
            server_send_unix_nanos: body.server_send_unix_nano,
            local_receive_unix_nanos,
            rtt_nanos,
            estimated_offset_nanos,
            quality: SampleQuality::Good,
        })
    }
}

/// Appends the `time` endpoint segment to a backend base URL.
///
/// The base path is preserved (so a `/chronos` prefix yields `/chronos/time`);
/// a trailing slash on the base is collapsed rather than producing `//time`.
fn time_endpoint_url(base_url: &Url) -> anyhow::Result<Url> {
    let mut url = base_url.clone();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| anyhow::anyhow!("backend base URL cannot have path segments"))?;
        // A trailing slash yields a trailing empty segment; drop it so the join
        // produces `<base>/time` rather than `<base>//time`.
        segments.pop_if_empty();
        segments.push("time");
    }
    Ok(url)
}

/// Classifies a backend URL into a [`BackendTransport`] for policy evaluation.
fn classify_transport(url: &Url) -> BackendTransport {
    if url.scheme() == "https" {
        BackendTransport::Https
    } else if is_loopback_host(url) {
        BackendTransport::PlainHttpLoopback
    } else {
        BackendTransport::PlainHttpRemote
    }
}

/// Returns whether the URL host is a loopback address or `localhost`.
fn is_loopback_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Maps a `reqwest` send error to a TLS- or generic-HTTP sample quality.
///
/// `reqwest` does not expose a dedicated TLS-error predicate, so the error and
/// its source chain are inspected for TLS-related wording.
fn classify_request_error(err: &reqwest::Error) -> SampleQuality {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(inner) = source {
        message.push_str("; ");
        message.push_str(&inner.to_string());
        source = inner.source();
    }
    let message = message.to_ascii_lowercase();
    if message.contains("certificate")
        || message.contains("tls")
        || message.contains("handshake")
        || message.contains("rustls")
    {
        SampleQuality::TlsError
    } else {
        SampleQuality::HttpError
    }
}

/// Returns the current wall-clock time as Unix nanoseconds.
fn wall_now_nanos() -> i128 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(delta) => delta.as_nanos() as i128,
        Err(err) => -(err.duration().as_nanos() as i128),
    }
}

/// Builds a `rustls` client config that performs standard validation plus SPKI
/// pinning against `pins`.
fn build_pinning_tls(pins: &[String]) -> anyhow::Result<rustls::ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let inner = WebPkiServerVerifier::builder(Arc::new(roots))
        .build()
        .context("building webpki verifier")?;
    let verifier = Arc::new(SpkiPinningVerifier {
        inner,
        pins: pins.to_vec(),
    });
    Ok(rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

/// Computes the base64-encoded SHA-256 of a certificate's SubjectPublicKeyInfo.
///
/// This is the value compared against `security.pinned_spki` entries; it matches
/// the output of `openssl x509 -pubkey … | openssl pkey -pubin -outform der |
/// openssl dgst -sha256 -binary | openssl base64`.
fn spki_sha256_base64(cert_der: &[u8]) -> Result<String, String> {
    use x509_parser::prelude::FromDer;
    let (_, cert) = x509_parser::certificate::X509Certificate::from_der(cert_der)
        .map_err(|err| format!("parsing certificate: {err}"))?;
    let mut hasher = Sha256::new();
    hasher.update(cert.tbs_certificate.subject_pki.raw);
    Ok(BASE64_STANDARD.encode(hasher.finalize()))
}

/// A `rustls` verifier that wraps standard validation and enforces SPKI pinning.
#[derive(Debug)]
struct SpkiPinningVerifier {
    inner: Arc<WebPkiServerVerifier>,
    pins: Vec<String>,
}

impl ServerCertVerifier for SpkiPinningVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )?;
        let pin = spki_sha256_base64(end_entity.as_ref()).map_err(rustls::Error::General)?;
        if self.pins.iter().any(|expected| expected == &pin) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "server SPKI does not match any configured pin".to_string(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn loopback_backend(base_url: String) -> BackendConfig {
        BackendConfig {
            name: "lab".to_string(),
            base_url,
            require_tls: false,
            require_valid_cert: false,
        }
    }

    fn time_body(sync: &str) -> serde_json::Value {
        let now = wall_now_nanos();
        serde_json::json!({
            "version": 1,
            "unix_sec": (now / 1_000_000_000) as i64,
            "unix_nano": now,
            "server_recv_unix_nano": now,
            "server_send_unix_nano": now,
            "status": {
                "provider": "chrony",
                "sync": sync,
                "stratum": 3,
                "last_offset_nanos": 1000
            }
        })
    }

    // PEM of the project's self-signed test certificate (CN=localhost). Its SPKI
    // pin is computed with openssl and asserted below.
    const TEST_CERT_PEM: &str = "\
-----BEGIN CERTIFICATE-----
MIIDJzCCAg+gAwIBAgIURpBLbeHa1+t21sYhRhr9emxVO1MwDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MCAXDTI2MDYxOTE1MDAzNVoYDzIxMjYw
NTI2MTUwMDM1WjAUMRIwEAYDVQQDDAlsb2NhbGhvc3QwggEiMA0GCSqGSIb3DQEB
AQUAA4IBDwAwggEKAoIBAQCpYJmVXcB5OjAK6O2tCIuNZ7r6iKLAvY0FyfvyrkF4
/znyraKgLnmNEz6qCH+CrQ+NdcmrTsb0AtZVrekzluJvnc9tMNbx/vZhdoVY/nPC
lhgnYX+l0ZGTIaIo/zp1cBgdKiyhTttC3vOiyXyDXxum/qZS7Sw42v82HI7iJjKg
fP/g2MOagdU6zbL2RfsOUKU1AaeUVhcdLlVKw84rzLcs5esHPr6b7ox6JdtXJl06
h6eUA8OukJVnphY61QNtU99DrbGtnRcZ+yEX4Sz+plPVyIYlr5HR7uSveI1EwOW1
eDg6dxdginUfWl9JhXciJGcf+E0rNt8c3oBG300dbDetAgMBAAGjbzBtMB0GA1Ud
DgQWBBQ0ANtQSRIveq+bmGf3nMhpf874JDAfBgNVHSMEGDAWgBQ0ANtQSRIveq+b
mGf3nMhpf874JDAPBgNVHRMBAf8EBTADAQH/MBoGA1UdEQQTMBGHBH8AAAGCCWxv
Y2FsaG9zdDANBgkqhkiG9w0BAQsFAAOCAQEADvNzE9ok2hVmxrrbctPy9NI4+mx+
SpopMQedvBW9ZuAwemHWKGkp9gv9f2YWD8qSYcnuh2UW50ywbaCMstMQg6V5IuFh
iIOtOLl/nrZvz5KeI7iKrtjxBTvx1aVfqXr+xMdpHdeto0mH02jyYjVKT5s9Dfbu
n3kbxt8CDxwLgTUSiD7n6Ha4FXV780nEMsQgTtPNG/nm1LgUxgMSw6s6KSGruR3a
w52xPRebURVaJwv5tZKLsoNnsW+KlelT69/rP3uC5HleHjx1pTqq46Bd4YfC+cCZ
mMUPKpNMKl6dXU2OWoxqBH1nMJ+QhnUDRPeJGz8ifDFrto3pSF9t63GORQ==
-----END CERTIFICATE-----
";

    fn test_cert_der() -> Vec<u8> {
        let body: String = TEST_CERT_PEM
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        BASE64_STANDARD.decode(body).expect("decode test cert")
    }

    #[test]
    fn computes_known_spki_pin() {
        let pin = spki_sha256_base64(&test_cert_der()).expect("spki pin");
        assert_eq!(pin, "ToXX6YsrFz2DC4I6K/IxLdW9np+HHirOUAfxobC/jCI=");
    }

    #[test]
    fn https_backend_with_pins_builds_client() {
        let config = BackendConfig {
            name: "primary".to_string(),
            base_url: "https://time.example.com".to_string(),
            require_tls: true,
            require_valid_cert: true,
        };
        let pins = vec!["ToXX6YsrFz2DC4I6K/IxLdW9np+HHirOUAfxobC/jCI=".to_string()];
        assert!(BackendClient::new(
            &config,
            SecurityPolicy::default(),
            Duration::from_secs(2),
            &pins
        )
        .is_ok());
    }

    #[test]
    fn time_endpoint_url_appends_time_to_base() {
        let cases = [
            ("https://time.example.com", "https://time.example.com/time"),
            ("https://time.example.com/", "https://time.example.com/time"),
            (
                "https://time.example.com/chronos",
                "https://time.example.com/chronos/time",
            ),
            (
                "https://time.example.com/chronos/",
                "https://time.example.com/chronos/time",
            ),
        ];
        for (base, expected) in cases {
            let url = time_endpoint_url(&Url::parse(base).unwrap()).expect("join");
            assert_eq!(url.as_str(), expected, "base {base}");
        }
    }

    #[tokio::test]
    async fn fetch_targets_prefixed_time_path() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/chronos/time"))
            .respond_with(ResponseTemplate::new(200).set_body_json(time_body("synchronized")))
            .mount(&server)
            .await;

        let config = loopback_backend(format!("{}/chronos", server.uri()));
        let client = BackendClient::new(
            &config,
            SecurityPolicy::default(),
            Duration::from_secs(2),
            &[],
        )
        .unwrap();
        let sample = client.fetch_sample().await.expect("good sample");
        assert_eq!(sample.quality, SampleQuality::Good);
    }

    #[test]
    fn classifies_transport_from_url() {
        assert_eq!(
            classify_transport(&Url::parse("https://time.example.com/time").unwrap()),
            BackendTransport::Https
        );
        assert_eq!(
            classify_transport(&Url::parse("http://127.0.0.1:8080/time").unwrap()),
            BackendTransport::PlainHttpLoopback
        );
        assert_eq!(
            classify_transport(&Url::parse("http://localhost/time").unwrap()),
            BackendTransport::PlainHttpLoopback
        );
        assert_eq!(
            classify_transport(&Url::parse("http://192.168.100.10:8080/time").unwrap()),
            BackendTransport::PlainHttpRemote
        );
    }

    #[test]
    fn remote_http_rejected_by_default_policy() {
        let config = BackendConfig {
            name: "remote".to_string(),
            base_url: "http://192.168.100.10:8080".to_string(),
            require_tls: false,
            require_valid_cert: false,
        };
        let result = BackendClient::new(
            &config,
            SecurityPolicy::default(),
            Duration::from_secs(2),
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn remote_http_allowed_when_policy_permits() {
        let config = BackendConfig {
            name: "remote".to_string(),
            base_url: "http://192.168.100.10:8080".to_string(),
            require_tls: false,
            require_valid_cert: false,
        };
        let policy = SecurityPolicy {
            allow_plain_http_backends: true,
            allow_plain_http_loopback: true,
        };
        assert!(BackendClient::new(&config, policy, Duration::from_secs(2), &[]).is_ok());
    }

    #[test]
    fn require_tls_rejects_plain_http() {
        let config = BackendConfig {
            name: "primary".to_string(),
            base_url: "http://127.0.0.1:8080".to_string(),
            require_tls: true,
            require_valid_cert: true,
        };
        assert!(BackendClient::new(
            &config,
            SecurityPolicy::default(),
            Duration::from_secs(2),
            &[]
        )
        .is_err());
    }

    #[tokio::test]
    async fn fetch_returns_good_sample_for_synchronized_backend() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/time"))
            .respond_with(ResponseTemplate::new(200).set_body_json(time_body("synchronized")))
            .mount(&server)
            .await;

        let config = loopback_backend(server.uri());
        let client = BackendClient::new(
            &config,
            SecurityPolicy::default(),
            Duration::from_secs(2),
            &[],
        )
        .unwrap();
        let sample = client.fetch_sample().await.expect("good sample");
        assert_eq!(sample.quality, SampleQuality::Good);
        assert_eq!(sample.backend_name, "lab");
    }

    #[tokio::test]
    async fn fetch_rejects_unsynchronized_backend() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/time"))
            .respond_with(ResponseTemplate::new(200).set_body_json(time_body("unsynchronized")))
            .mount(&server)
            .await;

        let config = loopback_backend(server.uri());
        let client = BackendClient::new(
            &config,
            SecurityPolicy::default(),
            Duration::from_secs(2),
            &[],
        )
        .unwrap();
        assert_eq!(
            client.fetch_sample().await.unwrap_err(),
            SampleQuality::BackendUnsynchronized
        );
    }

    #[tokio::test]
    async fn fetch_rejects_http_error_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/time"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let config = loopback_backend(server.uri());
        let client = BackendClient::new(
            &config,
            SecurityPolicy::default(),
            Duration::from_secs(2),
            &[],
        )
        .unwrap();
        assert_eq!(
            client.fetch_sample().await.unwrap_err(),
            SampleQuality::HttpError
        );
    }

    #[tokio::test]
    async fn fetch_rejects_invalid_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/time"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let config = loopback_backend(server.uri());
        let client = BackendClient::new(
            &config,
            SecurityPolicy::default(),
            Duration::from_secs(2),
            &[],
        )
        .unwrap();
        assert_eq!(
            client.fetch_sample().await.unwrap_err(),
            SampleQuality::InvalidResponse
        );
    }
}
