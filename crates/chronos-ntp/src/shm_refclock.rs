//! Memory layout and field encoding of ntpd/ntpsec's SHM refclock segment.
//!
//! The `SHM(u)` driver in ntpd/ntpsec exchanges time through a SysV shared
//! memory segment whose layout is the C `struct shmTime`. Both the daemon and
//! the writer run on the same host, so the segment uses the host's native
//! integer sizes and byte order. The layout below targets the common x86-64 /
//! aarch64 Linux ABI where `time_t` is an 8-byte signed integer, giving a
//! 96-byte struct (92 bytes of fields rounded up to the 8-byte alignment):
//!
//! ```text
//! offset  size  field
//!   0      4    mode                    (int)
//!   4      4    count                   (int, volatile)
//!   8      8    clockTimeStampSec       (time_t)
//!  16      4    clockTimeStampUSec      (int)
//!  20      4    (padding)
//!  24      8    receiveTimeStampSec     (time_t)
//!  32      4    receiveTimeStampUSec    (int)
//!  36      4    leap                    (int)
//!  40      4    precision               (int)
//!  44      4    nsamples                (int)
//!  48      4    valid                   (int, volatile)
//!  52      4    clockTimeStampNSec      (unsigned)
//!  56      4    receiveTimeStampNSec    (unsigned)
//!  60     32    dummy[8]                (int[8])
//! ```

use std::ptr;

use chronos_core::TimeSample;
use libc::{c_int, c_uint};

/// Base SysV IPC key for the SHM refclock; unit `u` uses `NTP_SHM_KEY_BASE + u`.
///
/// The value is `0x4E54_5030`, the ASCII bytes `"NTP0"`, matching ntpd/ntpsec.
pub const NTP_SHM_KEY_BASE: libc::key_t = 0x4E54_5030;

/// Size in bytes of the [`ShmTime`] segment on the target ABI.
pub const SHM_TIME_SIZE: usize = std::mem::size_of::<ShmTime>();

/// The shared-memory record read by ntpd/ntpsec's `SHM` refclock driver.
///
/// Field names and order mirror the upstream C `struct shmTime`; `#[repr(C)]`
/// reproduces the native ABI layout (including the implicit padding before
/// `receive_time_stamp_sec`) so the daemon reads the fields correctly.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ShmTime {
    /// Synchronization mode; `1` selects the `count`-based handshake.
    pub mode: c_int,
    /// Incremented before and after each update so readers detect torn writes.
    pub count: c_int,
    /// Reference (true) time, seconds component.
    ///
    /// `i64` matches the width of C `time_t` on the supported 64-bit Linux ABI;
    /// using the fixed-width type avoids depending on the deprecated
    /// `libc::time_t` alias while keeping the layout identical.
    pub clock_time_stamp_sec: i64,
    /// Reference (true) time, microseconds component.
    pub clock_time_stamp_usec: c_int,
    /// Local receive time, seconds component (C `time_t`; see
    /// [`ShmTime::clock_time_stamp_sec`]).
    pub receive_time_stamp_sec: i64,
    /// Local receive time, microseconds component.
    pub receive_time_stamp_usec: c_int,
    /// Leap-second indicator; zero means normal.
    pub leap: c_int,
    /// Clock precision as a power-of-two exponent of seconds.
    pub precision: c_int,
    /// Number of samples; unused by this writer and left zero.
    pub nsamples: c_int,
    /// Set to `1` once a complete sample is published, `0` while writing.
    pub valid: c_int,
    /// Reference (true) time, nanoseconds component.
    pub clock_time_stamp_nsec: c_uint,
    /// Local receive time, nanoseconds component.
    pub receive_time_stamp_nsec: c_uint,
    /// Reserved padding mirrored from the upstream struct.
    pub dummy: [c_int; 8],
}

/// Splits a Unix-nanosecond instant into whole seconds and the nanosecond
/// remainder in `[0, 1_000_000_000)`, using Euclidean division so negative
/// instants still yield a non-negative nanosecond component.
fn split_unix_nanos(nanos: i128) -> (i64, u32) {
    let sec = nanos.div_euclid(1_000_000_000);
    let nsec = nanos.rem_euclid(1_000_000_000);
    (sec as i64, nsec as u32)
}

/// Publishes `sample` into the `shmTime` record at `ptr` using ntpd's handshake.
///
/// The reference time is `receive + estimated_offset` so the daemon observes
/// the gateway's `remote - local` offset. The `count` field is bumped before
/// and after the field writes and `valid` is toggled around them, so a
/// concurrently reading daemon never consumes a torn sample.
///
/// # Safety
///
/// `ptr` must point to a valid, writable [`ShmTime`] that stays mapped for the
/// duration of the call. Writes are volatile because the segment is shared with
/// another process. The caller must ensure no other thread writes concurrently
/// (Chronos uses a single writer).
pub unsafe fn publish_sample(ptr: *mut ShmTime, sample: &TimeSample, precision: c_int) {
    let receive = sample.local_receive_unix_nanos;
    let clock = receive + sample.estimated_offset_nanos;
    let (clock_sec, clock_nsec) = split_unix_nanos(clock);
    let (recv_sec, recv_nsec) = split_unix_nanos(receive);

    let count_ptr = ptr::addr_of_mut!((*ptr).count);
    let valid_ptr = ptr::addr_of_mut!((*ptr).valid);

    ptr::write_volatile(ptr::addr_of_mut!((*ptr).mode), 1);
    ptr::write_volatile(valid_ptr, 0);
    ptr::write_volatile(count_ptr, ptr::read_volatile(count_ptr).wrapping_add(1));
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    ptr::write_volatile(ptr::addr_of_mut!((*ptr).clock_time_stamp_sec), clock_sec);
    ptr::write_volatile(
        ptr::addr_of_mut!((*ptr).clock_time_stamp_usec),
        (clock_nsec / 1_000) as c_int,
    );
    ptr::write_volatile(
        ptr::addr_of_mut!((*ptr).clock_time_stamp_nsec),
        clock_nsec as c_uint,
    );
    ptr::write_volatile(ptr::addr_of_mut!((*ptr).receive_time_stamp_sec), recv_sec);
    ptr::write_volatile(
        ptr::addr_of_mut!((*ptr).receive_time_stamp_usec),
        (recv_nsec / 1_000) as c_int,
    );
    ptr::write_volatile(
        ptr::addr_of_mut!((*ptr).receive_time_stamp_nsec),
        recv_nsec as c_uint,
    );
    ptr::write_volatile(ptr::addr_of_mut!((*ptr).leap), 0);
    ptr::write_volatile(ptr::addr_of_mut!((*ptr).precision), precision);

    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
    ptr::write_volatile(count_ptr, ptr::read_volatile(count_ptr).wrapping_add(1));
    ptr::write_volatile(valid_ptr, 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    use chronos_core::SampleQuality;

    #[test]
    fn struct_matches_native_abi_layout() {
        assert_eq!(std::mem::size_of::<ShmTime>(), 96);
        assert_eq!(offset_of!(ShmTime, mode), 0);
        assert_eq!(offset_of!(ShmTime, count), 4);
        assert_eq!(offset_of!(ShmTime, clock_time_stamp_sec), 8);
        assert_eq!(offset_of!(ShmTime, clock_time_stamp_usec), 16);
        assert_eq!(offset_of!(ShmTime, receive_time_stamp_sec), 24);
        assert_eq!(offset_of!(ShmTime, receive_time_stamp_usec), 32);
        assert_eq!(offset_of!(ShmTime, leap), 36);
        assert_eq!(offset_of!(ShmTime, precision), 40);
        assert_eq!(offset_of!(ShmTime, nsamples), 44);
        assert_eq!(offset_of!(ShmTime, valid), 48);
        assert_eq!(offset_of!(ShmTime, clock_time_stamp_nsec), 52);
        assert_eq!(offset_of!(ShmTime, receive_time_stamp_nsec), 56);
        assert_eq!(offset_of!(ShmTime, dummy), 60);
    }

    #[test]
    fn splits_unix_nanos_into_seconds_and_remainder() {
        assert_eq!(
            split_unix_nanos(1_781_844_000_123_456_789),
            (1_781_844_000, 123_456_789)
        );
        assert_eq!(split_unix_nanos(-1), (-1, 999_999_999));
    }

    #[test]
    fn publishes_offset_relative_to_receive_time() {
        // SAFETY: `shm` is a valid, owned, zero-initialized `ShmTime`.
        let mut shm: ShmTime = unsafe { std::mem::zeroed() };
        let sample = TimeSample {
            backend_name: "primary".to_string(),
            server_send_unix_nanos: 0,
            local_receive_unix_nanos: 1_781_844_000_000_000_000,
            rtt_nanos: 1_000,
            estimated_offset_nanos: 12_000_000,
            quality: SampleQuality::Good,
        };
        // SAFETY: `&mut shm` is a unique, valid pointer for the call's duration.
        unsafe { publish_sample(&mut shm, &sample, -1) };

        assert_eq!(shm.mode, 1);
        assert_eq!(shm.valid, 1);
        assert_eq!(shm.count, 2);
        assert_eq!(shm.precision, -1);
        assert_eq!(shm.receive_time_stamp_sec, 1_781_844_000);
        assert_eq!(shm.receive_time_stamp_nsec, 0);
        assert_eq!(shm.clock_time_stamp_sec, 1_781_844_000);
        assert_eq!(shm.clock_time_stamp_nsec, 12_000_000);
        assert_eq!(shm.clock_time_stamp_usec, 12_000);
    }
}
