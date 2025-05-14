use core::{mem, time::Duration};

use wdk_sys::{
    _KTIMER,
    _POOL_TYPE::NonPagedPoolNx,
    _TIMER_TYPE::{NotificationTimer, SynchronizationTimer},
    KTIMER, LARGE_INTEGER, PKTIMER, STATUS_INSUFFICIENT_RESOURCES,
    ntddk::{
        ExFreePoolWithTag, KeCancelTimer, KeInitializeTimerEx, KeReadStateTimer, KeSetTimerEx,
    },
};

use crate::{
    dpc::Dpc, kobject::Dispatchable, mutex::ex_allocate_pool_zero, ntstatus::NtError,
    raw::AsRawObject,
};

const TIMER_TAG: u32 = u32::from_ne_bytes(*b"rimt");

pub struct Timer {
    inner: PKTIMER,
    dpc: Dpc,
}

impl Timer {
    /// create a new `Timer`
    ///
    /// # Parameters
    /// - f: routine will be called when timer expired
    /// - is_synch: specify the type of timer, NotificationTimer or SynchronizationTimer this method will create
    pub fn new<F: Fn() + 'static>(f: F, is_synch: bool) -> Result<Self, NtError> {
        let layout =
            ex_allocate_pool_zero(NonPagedPoolNx, mem::size_of::<KTIMER>() as _, TIMER_TAG);

        if layout.is_null() {
            return Err(NtError::new(STATUS_INSUFFICIENT_RESOURCES));
        }

        unsafe {
            KeInitializeTimerEx(
                layout.cast(),
                if is_synch {
                    SynchronizationTimer
                } else {
                    NotificationTimer
                },
            );
        }

        Ok(Self {
            inner: layout.cast(),
            dpc: Dpc::new(f)?,
        })
    }

    pub fn get_state(&self) -> bool {
        unsafe { KeReadStateTimer(self.inner) != 0 }
    }

    /// start this timer
    /// # Parameters
    /// - after: start this timer after `after` period
    /// - period: specify how long the timer will expires
    pub fn start(&self, after: Duration, period: Duration) {
        let due_time = LARGE_INTEGER {
            QuadPart: after.as_millis() as _,
        };

        unsafe {
            KeSetTimerEx(
                self.inner,
                due_time,
                period.as_millis() as _,
                self.dpc.get(),
            );
        }
    }

    /// stop this timer
    pub fn stop(&self) {
        unsafe {
            KeCancelTimer(self.inner);
        }
    }
}

impl AsRawObject for Timer {
    type Target = _KTIMER;
    fn as_raw(&self) -> *mut Self::Target {
        self.inner
    }
}

impl Dispatchable for Timer {}

impl Drop for Timer {
    fn drop(&mut self) {
        unsafe {
            ExFreePoolWithTag(self.inner.cast(), TIMER_TAG);
        }
    }
}

unsafe impl Send for Timer {}
unsafe impl Sync for Timer {}
