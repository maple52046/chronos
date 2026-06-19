//! The [`OutputBackend`] implementation that writes to chrony's SOCK refclock.

use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};

use chronos_core::{ChronosError, OutputBackend, TimeSample};

use crate::sock_refclock::SockSample;

/// Writes accepted samples to chrony's SOCK refclock Unix datagram socket.
///
/// chrony creates and owns the socket; this backend sends datagrams to it. It
/// never adjusts the system clock — `chronyd` remains the sole clock
/// disciplinarian.
#[derive(Debug, Clone)]
pub struct ChronySockRefclockBackend {
    sock_path: PathBuf,
}

impl ChronySockRefclockBackend {
    /// Creates a backend that sends samples to the socket at `sock_path`.
    #[must_use]
    pub fn new(sock_path: impl Into<PathBuf>) -> Self {
        Self {
            sock_path: sock_path.into(),
        }
    }

    /// Returns the configured socket path.
    #[must_use]
    pub fn sock_path(&self) -> &Path {
        &self.sock_path
    }
}

impl OutputBackend for ChronySockRefclockBackend {
    fn submit_sample(&self, sample: &TimeSample) -> Result<(), ChronosError> {
        let bytes = SockSample::from_time_sample(sample).encode();
        let socket = UnixDatagram::unbound().map_err(|err| {
            ChronosError::OutputBackend(format!("creating datagram socket: {err}"))
        })?;
        socket.send_to(&bytes, &self.sock_path).map_err(|err| {
            ChronosError::OutputBackend(format!(
                "sending sample to {}: {err}",
                self.sock_path.display()
            ))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chronos_core::SampleQuality;

    use crate::sock_refclock::{SOCK_MAGIC, SOCK_SAMPLE_LEN};

    fn unique_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("chronos-sock-{nanos}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn sends_encoded_sample_to_listening_socket() {
        let dir = unique_dir();
        let sock_path = dir.join("chrony.sock");
        let listener = UnixDatagram::bind(&sock_path).expect("bind listener");

        let backend = ChronySockRefclockBackend::new(&sock_path);
        let sample = TimeSample {
            backend_name: "primary".to_string(),
            server_send_unix_nanos: 0,
            local_receive_unix_nanos: 1_781_844_000_000_000_000,
            rtt_nanos: 1_000,
            estimated_offset_nanos: 12_000,
            quality: SampleQuality::Good,
        };
        backend.submit_sample(&sample).expect("submit sample");

        let mut buffer = [0u8; 64];
        let received = listener.recv(&mut buffer).expect("receive datagram");
        assert_eq!(received, SOCK_SAMPLE_LEN);
        assert_eq!(&buffer[36..40], &SOCK_MAGIC.to_ne_bytes());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn errors_when_socket_is_missing() {
        let backend = ChronySockRefclockBackend::new("/nonexistent/chronos/chrony.sock");
        let sample = TimeSample {
            backend_name: "primary".to_string(),
            server_send_unix_nanos: 0,
            local_receive_unix_nanos: 1_781_844_000_000_000_000,
            rtt_nanos: 1_000,
            estimated_offset_nanos: 0,
            quality: SampleQuality::Good,
        };
        assert!(backend.submit_sample(&sample).is_err());
    }
}
