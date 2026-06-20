//! Native HTTPS transport using `axum-server` and `rustls`.
//!
//! This is the "native HTTPS" deployment mode where `chronos-server` terminates
//! TLS itself. Reverse-proxy deployments leave TLS disabled and terminate it in
//! Nginx/Caddy/HAProxy instead.

use std::net::SocketAddr;
use std::path::Path;

use anyhow::Context;
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;

/// Serves `app` over HTTPS on `addr`, loading the PEM certificate and key.
///
/// # Errors
///
/// Returns an error if the certificate or key cannot be read or parsed, or if
/// binding the listener fails.
pub async fn serve_https(
    addr: SocketAddr,
    app: Router,
    cert_file: &Path,
    key_file: &Path,
) -> anyhow::Result<()> {
    let config = RustlsConfig::from_pem_file(cert_file, key_file)
        .await
        .with_context(|| {
            format!(
                "loading TLS material from {} and {}",
                cert_file.display(),
                key_file.display()
            )
        })?;
    axum_server::bind_rustls(addr, config)
        .serve(app.into_make_service())
        .await
        .context("serving HTTPS")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chronos_core::{BackendStatus, ChronosError, TimeProvider, TimeStatusProvider};

    use crate::api::{router, AppState};

    const TEST_CERT: &str = "\
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

    const TEST_KEY: &str = "\
-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCpYJmVXcB5OjAK
6O2tCIuNZ7r6iKLAvY0FyfvyrkF4/znyraKgLnmNEz6qCH+CrQ+NdcmrTsb0AtZV
rekzluJvnc9tMNbx/vZhdoVY/nPClhgnYX+l0ZGTIaIo/zp1cBgdKiyhTttC3vOi
yXyDXxum/qZS7Sw42v82HI7iJjKgfP/g2MOagdU6zbL2RfsOUKU1AaeUVhcdLlVK
w84rzLcs5esHPr6b7ox6JdtXJl06h6eUA8OukJVnphY61QNtU99DrbGtnRcZ+yEX
4Sz+plPVyIYlr5HR7uSveI1EwOW1eDg6dxdginUfWl9JhXciJGcf+E0rNt8c3oBG
300dbDetAgMBAAECggEAA4P4BBfjThDVXPCbOEdwYBG6WYda+22jvu5DjrSrsrd3
bDBK6xCz4Kf42b4d6WgupbS/aBEVQ4zIhpQ6viGgVwk7PCyylWjBk+HKID/9xpjn
bO/S3q30G83rp/auL7BRK8/Lh9iTZ/apL2SHs1FWyVdJO/jkvVRcTjL3Cz4YOGj/
IEL2gUfEwgESr39SFpSVEU1QumIh6SU+2jACgdWiuNfUCMvfGcjpbv5wNNNgSW36
IWamiRv7vMi78swSQx4AoCMJZ794/+Vp7k7FdMCKJsaJo9miJQlHhEgo8/NMwgOO
FcZZh8tFSj8sZw5AFOlXtgT76vDt5KyaRmT3wAZmwQKBgQDUlKTd72mr2K3HOVkF
dnkjvYZ6/iyBwQWBhAyjLSXMZ76Yg4Tvz0j1RANSDV4Vll6e//fuNBJeuskEVy86
iNbb/WqklL9HsvJcwV0d70WXtad7m03TW6dq+zENRT8I2SGopEfDI+APVec8jzGA
m2cjdXUv21s9tKvnEXRBDjCQQQKBgQDL+PRIPV5yisLF2W/WOrKLIj+OUccZYKWa
z6JJv2/PiDbTLF8RQHtkglVQEpO0IOM/LfEG+TIRdIzCLivd12fXcczCU+/41NGQ
BgWh5jWzvQF8hGbrpTPYc6VqPXjKN4JwqOBGPGu2ARHm4kq3HomUhjMkGeuQiygd
p8xp7QvMbQKBgQCGXa4X2cwhIsQ0uLrcCRZo0NK/ywi2uxqP0rdqLI9HtAt0uyy7
p3mmDWdL5cciPflw5rn/hkpWNhn49GKVzQiB5JwTizGcUC+BEXeaBDMowVkrd+6p
ObWImV1X1f0RyqzLu4rgfTySdOnEDX5sm82FTCjWJwB08E33r8Cbnix0AQKBgQCh
bhsHwWoqhR+5rmkQx8veyfA7FwXYU+E9MO9kJpq7STb60rc61CnUVkJm6Qz1FsJD
knyb5EV4AyIT3K7jGEQbAvnnIr9d1DE3J0z14VjHM+MlRTSfc1QhDwXvm08p29zk
hQNbEx68IQSmEprHaufAIKYBAHKrJzdMoSzq+KzJ0QKBgB6v0wgxZ79andZRFtLH
aiKado0stahzeZ7U6mZClt9l0cNRcaH6JnJjzIv7lkmXJaoe/ph0ZqEUu2sZLgHf
Q5msrvzxkYGevSo0qNEi2tieB9isAct/ZbcXgFVOyxBtoHjO6hdskdU9QqUNoSBg
sKwsujLyE0Ku/cUve8rRF7iq
-----END PRIVATE KEY-----
";

    struct SyncedProvider;

    impl TimeStatusProvider for SyncedProvider {
        fn provider(&self) -> TimeProvider {
            TimeProvider::Chrony
        }

        fn backend_status(&self) -> Result<BackendStatus, ChronosError> {
            Ok(BackendStatus::Synchronized {
                stratum: Some(2),
                last_offset_nanos: Some(1_000),
            })
        }
    }

    fn unique_temp(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}.pem"))
    }

    #[tokio::test]
    async fn https_listener_serves_time_over_tls() {
        let cert_path = unique_temp("chronos-cert");
        let key_path = unique_temp("chronos-key");
        std::fs::write(&cert_path, TEST_CERT).expect("write cert");
        std::fs::write(&key_path, TEST_KEY).expect("write key");

        let state = AppState::new(Arc::new(SyncedProvider), "no-store", false);
        let app = router(state, "");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let addr = listener.local_addr().expect("addr");
        drop(listener);

        let cert = cert_path.clone();
        let key = key_path.clone();
        let server = tokio::spawn(async move { serve_https(addr, app, &cert, &key).await });

        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .expect("client");
        let mut body = serde_json::Value::Null;
        for _ in 0..100 {
            match client.get(format!("https://{addr}/time")).send().await {
                Ok(response) => {
                    body = response.json::<serde_json::Value>().await.expect("json");
                    break;
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
            }
        }

        assert_eq!(body["version"], 1);
        assert_eq!(body["status"]["sync"], "synchronized");

        server.abort();
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }
}
