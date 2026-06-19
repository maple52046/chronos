//! Wire encoding of chrony's SOCK refclock sample.
//!
//! chrony's `refclock SOCK` driver reads a fixed C `struct sock_sample` from a
//! Unix datagram socket. The struct is read by raw memory copy, so its fields
//! use the host's native size and byte order. `chronos-gateway` and `chronyd`
//! always run on the same host, so native-endian encoding is correct here. The
//! layout below targets the common x86-64 / aarch64 Linux ABI where `time_t`
//! and `suseconds_t` are 8-byte signed integers, giving a 40-byte struct:
//!
//! ```text
//! offset  size  field
//!   0      8    struct timeval.tv_sec   (time_t,      i64)
//!   8      8    struct timeval.tv_usec  (suseconds_t, i64)
//!  16      8    offset                  (double,      f64)
//!  24      4    pulse                   (int,         i32)
//!  28      4    leap                    (int,         i32)
//!  32      4    _pad                    (int,         i32)
//!  36      4    magic                   (int,         i32)
//! ```

use chronos_core::TimeSample;

/// Total size in bytes of the encoded `sock_sample` struct.
pub const SOCK_SAMPLE_LEN: usize = 40;

/// Magic value chrony requires in every sample (`"SOCK"` in ASCII).
pub const SOCK_MAGIC: i32 = 0x534F_434B;

/// A chrony SOCK refclock sample, prior to native-endian encoding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SockSample {
    /// Local-clock seconds component at which the sample is valid.
    pub tv_sec: i64,
    /// Local-clock microseconds component at which the sample is valid.
    pub tv_usec: i64,
    /// Offset in seconds, by the documented convention `remote_time - local_time`.
    ///
    /// The sign convention is asserted against a real `chronyd` during lab
    /// acceptance (M7/M8); see `docs/chrony-integration.md`.
    pub offset_seconds: f64,
    /// Pulse flag; always zero for a plain time sample (no PPS).
    pub pulse: i32,
    /// Leap-second indicator; zero means normal.
    pub leap: i32,
}

impl SockSample {
    /// Builds a sample from a domain [`TimeSample`].
    ///
    /// The validity timestamp is the local receive time; the offset is the
    /// estimated `remote - local` offset converted to seconds.
    #[must_use]
    pub fn from_time_sample(sample: &TimeSample) -> Self {
        let local = sample.local_receive_unix_nanos;
        Self {
            tv_sec: (local / 1_000_000_000) as i64,
            tv_usec: ((local % 1_000_000_000) / 1_000) as i64,
            offset_seconds: sample.estimated_offset_nanos as f64 / 1e9,
            pulse: 0,
            leap: 0,
        }
    }

    /// Encodes the sample into its 40-byte native-endian wire representation.
    #[must_use]
    pub fn encode(&self) -> [u8; SOCK_SAMPLE_LEN] {
        let mut buffer = [0u8; SOCK_SAMPLE_LEN];
        buffer[0..8].copy_from_slice(&self.tv_sec.to_ne_bytes());
        buffer[8..16].copy_from_slice(&self.tv_usec.to_ne_bytes());
        buffer[16..24].copy_from_slice(&self.offset_seconds.to_ne_bytes());
        buffer[24..28].copy_from_slice(&self.pulse.to_ne_bytes());
        buffer[28..32].copy_from_slice(&self.leap.to_ne_bytes());
        // Bytes 32..36 are the explicit `_pad` field and stay zero.
        buffer[36..40].copy_from_slice(&SOCK_MAGIC.to_ne_bytes());
        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronos_core::SampleQuality;

    #[test]
    fn encodes_exactly_forty_bytes_with_expected_layout() {
        let sample = SockSample {
            tv_sec: 1_781_844_000,
            tv_usec: 123_456,
            offset_seconds: 0.000_012,
            pulse: 0,
            leap: 0,
        };
        let bytes = sample.encode();
        assert_eq!(bytes.len(), SOCK_SAMPLE_LEN);
        assert_eq!(&bytes[0..8], &1_781_844_000i64.to_ne_bytes());
        assert_eq!(&bytes[8..16], &123_456i64.to_ne_bytes());
        assert_eq!(&bytes[16..24], &0.000_012f64.to_ne_bytes());
        assert_eq!(&bytes[24..28], &0i32.to_ne_bytes());
        assert_eq!(&bytes[28..32], &0i32.to_ne_bytes());
        assert_eq!(&bytes[32..36], &0i32.to_ne_bytes());
        assert_eq!(&bytes[36..40], &SOCK_MAGIC.to_ne_bytes());
    }

    #[test]
    fn derives_timeval_and_offset_from_time_sample() {
        let sample = TimeSample {
            backend_name: "primary".to_string(),
            server_send_unix_nanos: 0,
            local_receive_unix_nanos: 1_781_844_000_123_456_000,
            rtt_nanos: 1_000,
            estimated_offset_nanos: 12_000,
            quality: SampleQuality::Good,
        };
        let sock = SockSample::from_time_sample(&sample);
        assert_eq!(sock.tv_sec, 1_781_844_000);
        assert_eq!(sock.tv_usec, 123_456);
        assert!((sock.offset_seconds - 0.000_012).abs() < 1e-12);
    }
}
