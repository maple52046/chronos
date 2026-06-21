//! The [`OutputBackend`] implementation that writes ntpd/ntpsec's SHM refclock.

use std::io;
use std::ptr;

use chronos_core::{ChronosError, OutputBackend, TimeSample};
use libc::{c_int, c_void};

use crate::shm_refclock::{publish_sample, ShmTime, NTP_SHM_KEY_BASE, SHM_TIME_SIZE};

/// Owns a pointer into an attached SysV shared-memory segment.
///
/// The raw pointer is not `Send`/`Sync` on its own. It is sound to share here
/// because the segment stays mapped for the process lifetime and Chronos uses a
/// single writer driven by one scheduler task.
struct ShmSegment {
    ptr: *mut ShmTime,
}

// SAFETY: the pointer targets a process-wide shared-memory mapping that lives
// for the program's duration; the gateway never moves or unmaps it and only one
// task writes through it, so sharing the handle across threads is sound.
unsafe impl Send for ShmSegment {}
// SAFETY: see the `Send` justification; concurrent access is restricted to the
// volatile, single-writer publish protocol in `publish_sample`.
unsafe impl Sync for ShmSegment {}

/// Writes accepted samples into ntpd/ntpsec's `SHM(unit)` refclock segment.
///
/// The backend attaches (creating it if absent) the shared-memory segment keyed
/// by `unit` and publishes each sample with the daemon's count/valid handshake.
/// It never adjusts the system clock; the local NTP daemon disciplines it.
pub struct ShmRefclockBackend {
    unit: i32,
    precision: c_int,
    segment: ShmSegment,
}

impl ShmRefclockBackend {
    /// Attaches the SHM segment for `unit`, creating it with `perm` if absent.
    ///
    /// `perm` is the SysV permission bits applied on creation (e.g. `0o666`,
    /// which lets a non-root gateway write a segment the daemon also reads).
    /// `precision` is published as the refclock precision exponent.
    ///
    /// # Errors
    ///
    /// Returns [`ChronosError::OutputBackend`] if the segment cannot be obtained
    /// or attached.
    pub fn new(unit: i32, perm: i32, precision: i32) -> Result<Self, ChronosError> {
        let key = NTP_SHM_KEY_BASE + unit as libc::key_t;
        // SAFETY: `shmget` only reads its scalar arguments; it returns a segment
        // id or `-1` on failure, which is checked below.
        let id = unsafe { libc::shmget(key, SHM_TIME_SIZE, libc::IPC_CREAT | perm) };
        if id < 0 {
            return Err(ChronosError::OutputBackend(format!(
                "shmget(unit {unit}) failed: {}",
                io::Error::last_os_error()
            )));
        }
        // SAFETY: `id` is a valid segment id from the call above; a null address
        // lets the kernel choose the mapping; the result is checked against the
        // documented `(void *) -1` error sentinel before use.
        let addr = unsafe { libc::shmat(id, ptr::null(), 0) };
        if addr == (-1_isize) as *mut c_void {
            return Err(ChronosError::OutputBackend(format!(
                "shmat(unit {unit}) failed: {}",
                io::Error::last_os_error()
            )));
        }
        Ok(Self {
            unit,
            precision: precision as c_int,
            segment: ShmSegment { ptr: addr.cast() },
        })
    }

    /// Returns the SHM unit number this backend writes to.
    #[must_use]
    pub fn unit(&self) -> i32 {
        self.unit
    }
}

impl OutputBackend for ShmRefclockBackend {
    fn submit_sample(&self, sample: &TimeSample) -> Result<(), ChronosError> {
        // SAFETY: `segment.ptr` is a valid `ShmTime` mapping held for the
        // backend's lifetime, and the scheduler drives a single writer, so the
        // volatile publish protocol has no concurrent writer.
        unsafe { publish_sample(self.segment.ptr, sample, self.precision) };
        Ok(())
    }

    fn target_description(&self) -> String {
        format!("ntp shm unit {0} (127.127.28.{0})", self.unit)
    }
}
