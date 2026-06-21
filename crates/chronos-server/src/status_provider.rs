//! Time-status providers for `chronos-server`.
//!
//! Two providers are offered. The default `system` provider reads the kernel's
//! NTP synchronization state via `adjtimex(2)`, so it reflects whichever daemon
//! disciplines the clock (chrony, ntpd, systemd-timesyncd, ...) without any
//! external binary. The `chrony` provider speaks chrony's command protocol
//! directly over UDP, so it needs neither the `chronyc` binary nor a shell.

use std::net::{ToSocketAddrs, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chronos_core::{BackendStatus, ChronosError, TimeProvider, TimeStatusProvider};

/// A [`TimeStatusProvider`] backed by the kernel NTP clock discipline state.
///
/// Reads `adjtimex(2)` and maps the kernel sync flags to a [`BackendStatus`].
/// This is daemon-agnostic: any NTP implementation that maintains the kernel
/// clock status (chrony, ntpd, ntpsec, systemd-timesyncd) is reflected here.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClockStatusProvider;

impl TimeStatusProvider for SystemClockStatusProvider {
    fn provider(&self) -> TimeProvider {
        TimeProvider::SystemClock
    }

    fn backend_status(&self) -> Result<BackendStatus, ChronosError> {
        Ok(read_kernel_status())
    }
}

/// Reads the kernel NTP synchronization state on Linux.
#[cfg(target_os = "linux")]
fn read_kernel_status() -> BackendStatus {
    // SAFETY: `timex` is plain-old-data; zero-initialization is a valid value.
    let mut tx: libc::timex = unsafe { std::mem::zeroed() };
    // Constraint: `modes == 0` makes `adjtimex` a read-only query (no clock change).
    tx.modes = 0;
    // SAFETY: `adjtimex` only reads/writes the provided `timex` and returns the
    // clock state, or -1 on error; `tx` is a valid, owned, initialized struct.
    let state = unsafe { libc::adjtimex(&mut tx) };
    if state < 0 {
        return BackendStatus::Unknown;
    }
    if state == libc::TIME_ERROR || (tx.status & libc::STA_UNSYNC) != 0 {
        return BackendStatus::Unsynchronized;
    }
    // The kernel exposes no stratum and no chrony-style last offset.
    BackendStatus::Synchronized {
        stratum: None,
        last_offset_nanos: None,
    }
}

/// Non-Linux fallback: the kernel NTP state is not portably available.
#[cfg(not(target_os = "linux"))]
fn read_kernel_status() -> BackendStatus {
    BackendStatus::Unknown
}

/// A [`TimeStatusProvider`] that queries `chronyd` over its command protocol.
///
/// Speaks the chrony command-and-monitoring (candm) wire protocol directly,
/// avoiding any dependency on the `chronyc` binary. Monitoring requests such as
/// `tracking` are accepted from loopback by default, so `address` is usually
/// `127.0.0.1:323`; with host networking the in-container query reaches the host
/// `chronyd`.
#[derive(Debug, Clone)]
pub struct ChronyStatusProvider {
    address: String,
}

impl ChronyStatusProvider {
    /// Creates a provider that queries `chronyd` at `address` (host:port).
    #[must_use]
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
        }
    }
}

impl TimeStatusProvider for ChronyStatusProvider {
    fn provider(&self) -> TimeProvider {
        TimeProvider::Chrony
    }

    fn backend_status(&self) -> Result<BackendStatus, ChronosError> {
        let reply = self.query_tracking()?;
        parse_tracking_reply(&reply.bytes[..reply.len], reply.sequence)
            .map_err(ChronosError::TimeStatusUnavailable)
    }
}

/// A received reply buffer plus the sequence number that was requested.
struct Reply {
    bytes: [u8; 1024],
    len: usize,
    sequence: u32,
}

impl ChronyStatusProvider {
    /// Sends a `REQ_TRACKING` request and returns the raw reply.
    fn query_tracking(&self) -> Result<Reply, ChronosError> {
        let target = self
            .address
            .to_socket_addrs()
            .map_err(|err| {
                ChronosError::TimeStatusUnavailable(format!(
                    "resolving chrony address {}: {err}",
                    self.address
                ))
            })?
            .next()
            .ok_or_else(|| {
                ChronosError::TimeStatusUnavailable(format!(
                    "chrony address {} resolved to nothing",
                    self.address
                ))
            })?;

        let bind_addr = if target.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        let socket = UdpSocket::bind(bind_addr).map_err(|err| {
            ChronosError::TimeStatusUnavailable(format!("binding chrony query socket: {err}"))
        })?;
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .and_then(|()| socket.set_write_timeout(Some(Duration::from_secs(2))))
            .map_err(|err| {
                ChronosError::TimeStatusUnavailable(format!(
                    "configuring chrony query socket: {err}"
                ))
            })?;

        let sequence = request_sequence();
        let request = build_tracking_request(sequence);
        socket.send_to(&request, target).map_err(|err| {
            ChronosError::TimeStatusUnavailable(format!(
                "sending chrony request to {target}: {err}"
            ))
        })?;

        let mut bytes = [0u8; 1024];
        let len = socket.recv(&mut bytes).map_err(|err| {
            ChronosError::TimeStatusUnavailable(format!("reading chrony reply: {err}"))
        })?;
        Ok(Reply {
            bytes,
            len,
            sequence,
        })
    }
}

/// chrony command protocol version implemented here.
const PROTO_VERSION: u8 = 6;
/// Packet type for a command request.
const PKT_TYPE_CMD_REQUEST: u8 = 1;
/// Packet type for a command reply.
const PKT_TYPE_CMD_REPLY: u8 = 2;
/// `REQ_TRACKING` command code.
const REQ_TRACKING: u16 = 33;
/// `RPY_TRACKING` reply code.
const RPY_TRACKING: u16 = 5;
/// `STT_SUCCESS` status code.
const STT_SUCCESS: u16 = 0;

/// Builds a `REQ_TRACKING` request packet for protocol version 6.
///
/// The 20-byte header is followed by zero padding to at least the reply length;
/// protocol version 6 rejects requests shorter than their reply to prevent
/// traffic amplification, so the buffer is padded to 128 bytes.
fn build_tracking_request(sequence: u32) -> [u8; 128] {
    let mut buf = [0u8; 128];
    buf[0] = PROTO_VERSION;
    buf[1] = PKT_TYPE_CMD_REQUEST;
    // buf[2], buf[3] are reserved (zero).
    buf[4..6].copy_from_slice(&REQ_TRACKING.to_be_bytes());
    // buf[6..8] attempt = 0.
    buf[8..12].copy_from_slice(&sequence.to_be_bytes());
    // buf[12..20] pad1/pad2 = 0; remaining bytes are amplification padding.
    buf
}

/// Derives a request sequence number; uniqueness only needs to be best-effort.
fn request_sequence() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
}

/// Byte offsets of the fields read from an `RPY_Tracking` reply.
mod offset {
    /// Start of the reply-specific data union.
    pub const DATA: usize = 28;
    /// `stratum` (u16): data + ref_id(4) + ip_addr(20).
    pub const STRATUM: usize = DATA + 24;
    /// `leap_status` (u16): immediately after `stratum`.
    pub const LEAP_STATUS: usize = STRATUM + 2;
    /// `last_offset` (Float): leap(2) + ref_time(12) + current_correction(4).
    pub const LAST_OFFSET: usize = LEAP_STATUS + 2 + 12 + 4;
}

/// Parses a chrony `RPY_Tracking` reply into a [`BackendStatus`].
///
/// The authoritative sync signal is `leap_status`: `0`/`1`/`2` (normal or an
/// announced leap second) means synchronized, `3` means not synchronized, and
/// anything else is unknown.
fn parse_tracking_reply(buf: &[u8], expected_sequence: u32) -> Result<BackendStatus, String> {
    if buf.len() < offset::LEAP_STATUS + 2 {
        return Err(format!("chrony reply too short: {} bytes", buf.len()));
    }
    if buf[1] != PKT_TYPE_CMD_REPLY {
        return Err(format!("unexpected chrony packet type {}", buf[1]));
    }
    let reply = u16::from_be_bytes([buf[6], buf[7]]);
    let status = u16::from_be_bytes([buf[8], buf[9]]);
    let sequence = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);
    if sequence != expected_sequence {
        return Err("chrony reply sequence mismatch".to_string());
    }
    if reply != RPY_TRACKING {
        return Err(format!("unexpected chrony reply code {reply}"));
    }
    if status != STT_SUCCESS {
        return Err(format!("chrony reported status {status}"));
    }

    let stratum = u16::from_be_bytes([buf[offset::STRATUM], buf[offset::STRATUM + 1]]);
    let leap = u16::from_be_bytes([buf[offset::LEAP_STATUS], buf[offset::LEAP_STATUS + 1]]);
    let last_offset_nanos = buf
        .get(offset::LAST_OFFSET..offset::LAST_OFFSET + 4)
        .map(|raw| {
            let seconds = decode_chrony_float([raw[0], raw[1], raw[2], raw[3]]);
            (seconds * 1e9).round() as i128
        });

    match leap {
        0..=2 => Ok(BackendStatus::Synchronized {
            stratum: u8::try_from(stratum).ok(),
            last_offset_nanos,
        }),
        3 => Ok(BackendStatus::Unsynchronized),
        other => Err(format!("unknown chrony leap status {other}")),
    }
}

/// Decodes chrony's 32-bit floating-point wire format into an `f64`.
///
/// The value packs a 7-bit signed exponent in the high bits and a 25-bit signed
/// coefficient in the low bits, in network byte order.
fn decode_chrony_float(raw: [u8; 4]) -> f64 {
    const COEF_BITS: u32 = 25;
    const EXP_BITS: u32 = 7;
    let x = u32::from_be_bytes(raw);

    let mut exp = (x >> COEF_BITS) as i32;
    if exp >= 1 << (EXP_BITS - 1) {
        exp -= 1 << EXP_BITS;
    }
    exp -= COEF_BITS as i32;

    let mut coef = (x & ((1 << COEF_BITS) - 1)) as i32;
    if coef >= 1 << (COEF_BITS - 1) {
        coef -= 1 << COEF_BITS;
    }

    f64::from(coef) * (f64::from(exp)).exp2()
}

/// A [`TimeStatusProvider`] that always reports an unknown status.
///
/// Selected when the configured provider name is unrecognized, so the server
/// still serves time without claiming a sync state.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnknownStatusProvider;

impl TimeStatusProvider for UnknownStatusProvider {
    fn provider(&self) -> TimeProvider {
        TimeProvider::Unknown
    }

    fn backend_status(&self) -> Result<BackendStatus, ChronosError> {
        Ok(BackendStatus::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a synthetic `RPY_Tracking` reply with the given leap status,
    /// stratum, and sequence for offset-parsing tests.
    fn tracking_reply(sequence: u32, stratum: u16, leap: u16) -> Vec<u8> {
        let mut buf = vec![0u8; offset::LAST_OFFSET + 4 + 4];
        buf[0] = PROTO_VERSION;
        buf[1] = PKT_TYPE_CMD_REPLY;
        buf[6..8].copy_from_slice(&RPY_TRACKING.to_be_bytes());
        buf[8..10].copy_from_slice(&STT_SUCCESS.to_be_bytes());
        buf[16..20].copy_from_slice(&sequence.to_be_bytes());
        buf[offset::STRATUM..offset::STRATUM + 2].copy_from_slice(&stratum.to_be_bytes());
        buf[offset::LEAP_STATUS..offset::LEAP_STATUS + 2].copy_from_slice(&leap.to_be_bytes());
        buf
    }

    #[test]
    fn parses_synchronized_reply() {
        let buf = tracking_reply(42, 3, 0);
        let status = parse_tracking_reply(&buf, 42).expect("parse");
        assert_eq!(
            status,
            BackendStatus::Synchronized {
                stratum: Some(3),
                last_offset_nanos: Some(0),
            }
        );
    }

    #[test]
    fn parses_unsynchronized_reply() {
        let buf = tracking_reply(7, 0, 3);
        assert_eq!(
            parse_tracking_reply(&buf, 7).expect("parse"),
            BackendStatus::Unsynchronized
        );
    }

    #[test]
    fn rejects_sequence_mismatch() {
        let buf = tracking_reply(1, 2, 0);
        assert!(parse_tracking_reply(&buf, 2).is_err());
    }

    #[test]
    fn rejects_short_reply() {
        assert!(parse_tracking_reply(&[0u8; 8], 0).is_err());
    }

    #[test]
    fn request_has_version_and_command() {
        let req = build_tracking_request(0x0102_0304);
        assert_eq!(req[0], PROTO_VERSION);
        assert_eq!(req[1], PKT_TYPE_CMD_REQUEST);
        assert_eq!(u16::from_be_bytes([req[4], req[5]]), REQ_TRACKING);
        assert_eq!(
            u32::from_be_bytes([req[8], req[9], req[10], req[11]]),
            0x0102_0304
        );
    }

    #[test]
    fn decodes_zero_float() {
        assert_eq!(decode_chrony_float([0, 0, 0, 0]), 0.0);
    }
}
