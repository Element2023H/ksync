use core::{mem, ptr};
use core::mem::MaybeUninit;

use alloc::boxed::Box;
use wdk::nt_success;
use wdk_sys::ntddk::ObfDereferenceObject;
use wdk_sys::{
    _KWAIT_REASON::Executive,
    _MODE::KernelMode,
    _THREADINFOCLASS::ThreadBasicInformation,
    CLIENT_ID, FALSE, GENERIC_ALL, HANDLE, LONG, NTSTATUS, OBJ_KERNEL_HANDLE, PETHREAD, PULONG,
    PVOID, PsThreadType, STATUS_SUCCESS, THREAD_QUERY_LIMITED_INFORMATION, ULONG,
    ntddk::{KeWaitForSingleObject, ObReferenceObjectByHandle, PsCreateSystemThread, ZwClose},
};

use crate::NtCurrentProcess;
use crate::{
    initialize_object_attributes,
    ntstatus::{NtError, cvt},
};

#[repr(C)]
pub struct THREAD_BASIC_INFORMATION {
    pub ExitStatus: LONG,
    pub TebBaseAddress: PVOID,
    pub ClientId: CLIENT_ID,
    pub AffinityMask: usize,
    pub Priority: LONG,
    pub BasePriority: LONG,
}

impl Default for THREAD_BASIC_INFORMATION {
    fn default() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}

unsafe extern "C" {
    pub fn ZwQueryInformationThread(
        ThreadHandle: HANDLE,
        ThreadInformationClass: ULONG,
        ThreadInformation: PVOID,
        ThreadInformationLength: ULONG,
        ReturnLength: PULONG,
    ) -> NTSTATUS;

}

pub struct JoinHandle {
    handle: HANDLE,
    exit_status: Option<NTSTATUS>,
}

impl Default for JoinHandle {
    fn default() -> Self {
        Self { handle: ptr::null_mut(), exit_status: None }
    }
}

impl JoinHandle {
    pub fn dettach(&mut self) {
        let _ = unsafe { ZwClose(self.handle) };
        self.handle = ptr::null_mut();
    }

    pub fn joinable(&self) -> bool {
        !self.handle.is_null() && self.is_running()
    }

    pub fn join(&mut self) -> Result<(), NtError> {
        let mut thread: PVOID = ptr::null_mut();

        let mut status = unsafe {
            ObReferenceObjectByHandle(
                self.handle,
                THREAD_QUERY_LIMITED_INFORMATION,
                *PsThreadType,
                KernelMode as _,
                &mut thread,
                ptr::null_mut(),
            )
        };

        cvt(status)?;

        status = unsafe {
            KeWaitForSingleObject(
                thread,
                Executive as _,
                KernelMode as _,
                FALSE as _,
                ptr::null_mut(),
            )
        };

        cvt(status)?;

        unsafe { ObfDereferenceObject(thread) };

        // unconditionally set self.exit_status no matter a wait failure or a query failure occurrs
        let mut length: ULONG = 0;
        let mut info = THREAD_BASIC_INFORMATION::default();

        status = unsafe {
            ZwQueryInformationThread(
                self.handle,
                ThreadBasicInformation as _,
                &mut info as *mut _ as *mut _,
                mem::size_of::<THREAD_BASIC_INFORMATION>() as _,
                &mut length,
            )
        };

        cvt(status)?;

        self.exit_status = Some(info.ExitStatus);

        Ok(())
    }

    /// this method will return None if the thread is still running
    pub fn exit_status(&self) -> Option<NTSTATUS> {
        self.exit_status
    }

    pub fn is_running(&self) -> bool {
        self.exit_status.is_none()
    }
}

impl Drop for JoinHandle {
    fn drop(&mut self) {
        if self.joinable() {
            self.dettach();
        }
    }
}

extern "C" fn start_routine_stub<F: FnOnce()>(context: PVOID) {
    let ctx: Box<F> = unsafe { Box::from_raw(mem::transmute::<_, *mut F>(context)) };

    (*ctx)();
}

pub fn spawn<F: FnOnce()>(f: F) -> Result<JoinHandle, NtError> {
    let mut handle: HANDLE = ptr::null_mut();

    unsafe {
        let mut attr = initialize_object_attributes!(
            ptr::null_mut(),
            OBJ_KERNEL_HANDLE,
            ptr::null_mut(),
            ptr::null_mut()
        );

        let buf = Box::new(f);
        let context = Box::into_raw(buf);

        let status = PsCreateSystemThread(
            &mut handle,
            GENERIC_ALL,
            &mut attr,
            NtCurrentProcess,
            ptr::null_mut(),
            Some(start_routine_stub::<F>),
            context.cast(),
        );

        if !nt_success(status) {
            let _ = Box::from_raw(context);
            return Err(NtError::from(status));
        }
    }

    Ok(JoinHandle {
        handle,
        exit_status: None,
    })
}

pub mod this_thread {
    use core::{arch::x86_64::_mm_pause, time::Duration};

    use wdk_sys::{
        _MODE::KernelMode,
        FALSE, LARGE_INTEGER, ULONG,
        ntddk::{KeDelayExecutionThread, PsGetCurrentThreadId},
    };

    use crate::handle_to_ulong;

    pub fn sleep(ms: Duration) {
        let mut timeout = LARGE_INTEGER {
            QuadPart: -1 as i64 * 1_0000 * ms.as_millis() as i64,
        };

        unsafe {
            let _ = KeDelayExecutionThread(KernelMode as i8, FALSE as u8, &mut timeout);
        }
    }

    pub fn pause() {
        unsafe { _mm_pause() };
    }

    pub fn id() -> ULONG {
        unsafe { handle_to_ulong!(PsGetCurrentThreadId()) }
    }
}
