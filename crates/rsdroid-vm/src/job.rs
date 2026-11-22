//! Windows Job Object ownership for QEMU.
//!
//! The job has `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so when the last handle
//! closes (including a hard kill of the rsdroid process) Windows terminates QEMU.

#[cfg(windows)]
mod platform {
    use std::io;
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// A job object that kills its processes when dropped.
    pub struct JobObject {
        handle: HANDLE,
    }

    impl JobObject {
        /// Create a job with `KILL_ON_JOB_CLOSE`.
        pub fn new() -> io::Result<Self> {
            // SAFETY: null attributes and name are valid; the returned handle is
            // checked before use and owned by this struct.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self { handle };

            // SAFETY: the structure is zeroed, then the documented limit flag is
            // set. The size matches the structure the API expects.
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `job.handle` is valid and `info` lives for the call.
            let ok = unsafe {
                SetInformationJobObject(
                    job.handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }

        /// Put a spawned child into the job.
        pub fn assign(&self, child: &Child) -> io::Result<()> {
            // SAFETY: both handles are valid for the duration of the call.
            let ok =
                unsafe { AssignProcessToJobObject(self.handle, child.as_raw_handle() as HANDLE) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            // SAFETY: this struct owns the handle and closes it exactly once.
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::io;
    use std::process::Child;

    /// Non-Windows placeholder so the crate still compiles for documentation.
    pub struct JobObject;

    impl JobObject {
        /// Job Objects are only available on Windows.
        pub fn new() -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows Job Objects are required to contain QEMU",
            ))
        }

        /// Job Objects are only available on Windows.
        pub fn assign(&self, _child: &Child) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows Job Objects are required to contain QEMU",
            ))
        }
    }
}

pub use platform::JobObject;
