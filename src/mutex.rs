use crate::ntstatus::NtError;
use alloc::boxed::Box;
use core::{
    cell::UnsafeCell,
    fmt::{Debug, Display},
    mem::{self, MaybeUninit},
    ops::{Deref, DerefMut},
    ptr::{NonNull, drop_in_place},
};
use wdk_sys::{
    _EVENT_TYPE::SynchronizationEvent,
    _POOL_TYPE::NonPagedPoolNx,
    APC_LEVEL, DISPATCH_LEVEL, ERESOURCE, FALSE, FAST_MUTEX, FM_LOCK_BIT, KGUARDED_MUTEX, KIRQL,
    KLOCK_QUEUE_HANDLE, KSPIN_LOCK, PKLOCK_QUEUE_HANDLE, POOL_TYPE, PVOID, SIZE_T, STATUS_SUCCESS,
    STATUS_UNSUCCESSFUL, TRUE, ULONG,
    ntddk::{
        ExAcquireFastMutex, ExAcquireResourceExclusiveLite, ExAcquireResourceSharedLite,
        ExDeleteResourceLite, ExFreePoolWithTag, ExInitializeResourceLite, ExReleaseFastMutex,
        ExReleaseResourceLite, ExTryToAcquireFastMutex, KeAcquireGuardedMutex,
        KeAcquireInStackQueuedSpinLock, KeAcquireSpinLockRaiseToDpc, KeGetCurrentIrql,
        KeInitializeEvent, KeInitializeGuardedMutex, KeInitializeSpinLock, KeReleaseGuardedMutex,
        KeReleaseInStackQueuedSpinLock, KeReleaseSpinLock, KeTryToAcquireGuardedMutex,
        KeTryToAcquireSpinLockAtDpcLevel, memset,
    },
};

fn ExInitializeFastMutex(fast_mutex: *mut FAST_MUTEX) {
    unsafe {
        core::ptr::write_volatile(&mut (*fast_mutex).Count, FM_LOCK_BIT as i32);

        (*fast_mutex).Owner = core::ptr::null_mut();
        (*fast_mutex).Contention = 0;
        KeInitializeEvent(&mut (*fast_mutex).Event, SynchronizationEvent, FALSE as _)
    }
}

// out of fashion api collections
// TODO: move it out of this module
mod otf {
    use super::*;

    unsafe extern "C" {
        pub fn ExAllocatePoolWithTag(pool_type: POOL_TYPE, size: SIZE_T, tag: ULONG) -> PVOID;
    }

    pub fn ex_allocate_pool_zero(pool_type: POOL_TYPE, size: SIZE_T, tag: ULONG) -> PVOID {
        let ptr = unsafe { ExAllocatePoolWithTag(pool_type, size, tag) };

        if !ptr.is_null() {
            unsafe { memset(ptr, 0, size) };
        }

        ptr
    }
}

use otf::ex_allocate_pool_zero;

const MUTEX_TAG: ULONG = u32::from_ne_bytes(*b"xetm");

pub trait Mutex {
    type Target: Mutex;

    fn init(this: &mut Self::Target);

    fn lock(&self);

    fn trylock(&self) -> bool {
        false
    }

    fn lock_shared(&self) {}

    fn try_lock_shared(&self) -> bool {
        false
    }

    fn unlock_shared(&self) {}

    fn unlock(&self);

    fn irql_ok() -> bool {
        return unsafe { KeGetCurrentIrql() <= APC_LEVEL as u8 };
    }
}

pub trait QueuedMutex {
    type Target: QueuedMutex;

    fn init(this: &mut Self::Target);

    fn lock(&self, handle: PKLOCK_QUEUE_HANDLE);

    fn unlock(&self, handle: PKLOCK_QUEUE_HANDLE);

    fn irql_ok() -> bool {
        return unsafe { KeGetCurrentIrql() <= DISPATCH_LEVEL as u8 };
    }
}

pub struct EmptyMutex;

pub struct FastMutex {
    inner: UnsafeCell<FAST_MUTEX>,
}

pub struct GuardedMutex {
    inner: UnsafeCell<KGUARDED_MUTEX>,
}

pub struct ResourceMutex {
    inner: UnsafeCell<ERESOURCE>,
}

pub struct SpinMutex {
    inner: UnsafeCell<SpinLockInner>,
}

impl Mutex for EmptyMutex {
    type Target = Self;

    fn init(this: &mut Self::Target) {
        let _ = this;
    }

    fn lock(&self) {}

    fn unlock(&self) {}
}

impl Mutex for FastMutex {
    type Target = Self;

    fn init(this: &mut Self::Target) {
        ExInitializeFastMutex(this.inner.get());
    }

    fn lock(&self) {
        unsafe {
            ExAcquireFastMutex(self.inner.get());
        }
    }

    fn trylock(&self) -> bool {
        return unsafe { ExTryToAcquireFastMutex(self.inner.get()) != 0 };
    }

    fn unlock(&self) {
        unsafe { ExReleaseFastMutex(self.inner.get()) };
    }
}

impl Mutex for GuardedMutex {
    type Target = Self;

    fn init(this: &mut Self::Target) {
        unsafe { KeInitializeGuardedMutex(this.inner.get()) };
    }

    fn lock(&self) {
        unsafe {
            KeAcquireGuardedMutex(self.inner.get());
        }
    }

    fn trylock(&self) -> bool {
        return unsafe { KeTryToAcquireGuardedMutex(self.inner.get()) != 0 };
    }

    fn unlock(&self) {
        unsafe { KeReleaseGuardedMutex(self.inner.get()) };
    }
}

impl Mutex for ResourceMutex {
    type Target = Self;

    fn init(this: &mut Self::Target) {
        unsafe {
            match ExInitializeResourceLite(this.inner.get()) {
                STATUS_SUCCESS => (),
                _ => panic!("can not initialize ERESOURCE"),
            }
        }
    }

    fn lock(&self) {
        unsafe {
            ExAcquireResourceExclusiveLite(self.inner.get(), TRUE as _);
        }
    }

    fn trylock(&self) -> bool {
        return unsafe { ExAcquireResourceExclusiveLite(self.inner.get(), FALSE as _) != 0 };
    }

    fn lock_shared(&self) {
        unsafe {
            ExAcquireResourceSharedLite(self.inner.get(), TRUE as _);
        }
    }

    fn unlock_shared(&self) {
        unsafe {
            ExReleaseResourceLite(self.inner.get());
        }
    }

    fn try_lock_shared(&self) -> bool {
        unsafe { ExAcquireResourceSharedLite(self.inner.get(), FALSE as _) != 0 }
    }

    fn unlock(&self) {
        unsafe { ExReleaseResourceLite(self.inner.get()) };
    }
}

impl Drop for ResourceMutex {
    fn drop(&mut self) {
        unsafe {
            let _ = ExDeleteResourceLite(self.inner.get());
        }
    }
}

struct SpinLockInner {
    irql: KIRQL,
    lock: KSPIN_LOCK,
}

impl Mutex for SpinMutex {
    type Target = Self;

    fn init(this: &mut Self::Target) {
        unsafe { KeInitializeSpinLock(&mut (*this.inner.get()).lock) };
    }

    fn lock(&self) {
        unsafe {
            let inner = &mut (*self.inner.get());

            inner.irql = KeAcquireSpinLockRaiseToDpc(&mut inner.lock);
        }
    }

    fn trylock(&self) -> bool {
        unsafe { KeTryToAcquireSpinLockAtDpcLevel(&mut (*self.inner.get()).lock) != 0 }
    }

    fn unlock(&self) {
        unsafe {
            KeReleaseSpinLock(&mut (*self.inner.get()).lock, (*self.inner.get()).irql);
        }
    }

    // KeAcquireSpinLock can only be called at IRQL <= DISPATCH_LEVEL
    fn irql_ok() -> bool {
        unsafe { KeGetCurrentIrql() <= DISPATCH_LEVEL as u8 }
    }
}

impl QueuedMutex for QueuedSpinMutex {
    type Target = Self;

    fn init(this: &mut Self::Target) {
        unsafe {
            KeInitializeSpinLock(this.inner.get());
        }
    }

    fn lock(&self, handle: PKLOCK_QUEUE_HANDLE) {
        unsafe { KeAcquireInStackQueuedSpinLock(self.inner.get(), handle) }
    }

    fn unlock(&self, handle: PKLOCK_QUEUE_HANDLE) {
        unsafe {
            KeReleaseInStackQueuedSpinLock(handle);
        }
    }
}

struct InnerData<T, M: Mutex> {
    mutex: M::Target,
    data: T,
}

// a strategy lock wrapper for FastMutex, GuardMutex, Spinlock, Resources
pub struct Locked<T, M>
where
    M: Mutex,
{
    inner: NonNull<InnerData<T, M>>,
}

impl<T, M: Mutex> Locked<T, M> {
    pub fn new(data: T) -> Self {
        let buf = ex_allocate_pool_zero(
            NonPagedPoolNx,
            mem::size_of::<InnerData<T, M>>() as _,
            MUTEX_TAG,
        ) as *mut InnerData<T, M>;

        unsafe {
            (*buf).data = data;
            M::init(&mut (*buf).mutex);
        }

        Self {
            inner: NonNull::new(buf).unwrap(),
        }
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, T, M>, NtError> {
        if !M::irql_ok() {
            Err(NtError::from(STATUS_UNSUCCESSFUL))
        } else {
            unsafe { (*self.inner.as_ptr()).mutex.lock() };

            Ok(MutexGuard { mutex: self })
        }
    }

    pub fn lock_shared(&self) -> Result<MutexGuard<'_, T, M>, NtError> {
        if !M::irql_ok() {
            Err(NtError::from(STATUS_UNSUCCESSFUL))
        } else {
            unsafe { (*self.inner.as_ptr()).mutex.lock_shared() };

            Ok(MutexGuard { mutex: self })
        }
    }
}

impl<T, M: Mutex> Drop for Locked<T, M> {
    fn drop(&mut self) {
        unsafe {
            drop_in_place(&mut self.inner.as_mut().data);

            drop_in_place(&mut self.inner.as_mut().mutex);

            ExFreePoolWithTag(self.inner.as_ptr().cast(), MUTEX_TAG);
        }
    }
}

impl<T: Display, M: Mutex> Debug for Locked<T, M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Locked{{{}}}", unsafe { &(*self.inner.as_ptr()).data })
    }
}

pub struct MutexGuard<'a, T, M: Mutex> {
    mutex: &'a Locked<T, M>,
}

impl<'a, T, M: Mutex> Deref for MutexGuard<'a, T, M> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &self.mutex.inner.as_ref().data }
    }
}

impl<'a, T, M: Mutex> DerefMut for MutexGuard<'a, T, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut (*self.mutex.inner.as_ptr()).data }
    }
}

impl<'a, T, M: Mutex> Drop for MutexGuard<'a, T, M> {
    fn drop(&mut self) {
        unsafe {
            (*self.mutex.inner.as_ptr()).mutex.unlock();
        }
    }
}

pub struct QueuedEmptyMutex;

impl QueuedMutex for QueuedEmptyMutex {
    type Target = Self;

    fn init(this: &mut Self::Target) {
        let _ = this;
    }

    fn lock(&self, handle: PKLOCK_QUEUE_HANDLE) {
        let _ = handle;
    }

    fn unlock(&self, handle: PKLOCK_QUEUE_HANDLE) {
        let _ = handle;
    }
}

pub struct QueuedSpinMutex {
    inner: UnsafeCell<KSPIN_LOCK>,
}

struct QueuedInnerData<T, M: QueuedMutex> {
    mutex: M::Target,
    data: T,
}

/// a strategy lock wrapper for Queued Spin Locks
pub struct StackQueueLocked<T, M: QueuedMutex> {
    inner: NonNull<QueuedInnerData<T, M>>,
}

impl<T, M: QueuedMutex> StackQueueLocked<T, M> {
    pub fn new(data: T) -> Self {
        let buf = ex_allocate_pool_zero(
            NonPagedPoolNx,
            mem::size_of::<QueuedInnerData<T, M>>() as _,
            MUTEX_TAG,
        ) as *mut QueuedInnerData<T, M>;

        unsafe {
            (*buf).data = data;
            M::init(&mut (*buf).mutex);
        };

        Self {
            inner: NonNull::new(buf).unwrap(),
        }
    }

    pub fn lock<'a>(
        &'a self,
        handle: &'a mut LockedQuueHandle,
    ) -> Result<InStackMutexGuard<'a, T, M>, NtError> {
        if !M::irql_ok() {
            Err(NtError::from(STATUS_UNSUCCESSFUL))
        } else {
            unsafe { (*self.inner.as_ptr()).mutex.lock(&mut handle.0) };

            Ok(InStackMutexGuard {
                handle,
                mutex: self,
            })
        }
    }
}

impl<T: Display, M: QueuedMutex> Debug for StackQueueLocked<T, M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "StackQueueLocked{{{}}}", unsafe {
            &(*self.inner.as_ptr()).data
        })
    }
}

#[repr(C)]
pub struct LockedQuueHandle(KLOCK_QUEUE_HANDLE);

impl LockedQuueHandle {
    pub fn new() -> Self {
        Self(KLOCK_QUEUE_HANDLE::default())
    }
}

pub struct InStackMutexGuard<'a, T, M: QueuedMutex> {
    handle: &'a mut LockedQuueHandle,
    mutex: &'a StackQueueLocked<T, M>,
}

impl<'a, T, M: QueuedMutex> Deref for InStackMutexGuard<'a, T, M> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &self.mutex.inner.as_ref().data }
    }
}

impl<'a, T, M: QueuedMutex> DerefMut for InStackMutexGuard<'a, T, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut (*self.mutex.inner.as_ptr()).data }
    }
}

impl<'a, T, M: QueuedMutex> Drop for InStackMutexGuard<'a, T, M> {
    fn drop(&mut self) {
        unsafe {
            (*self.mutex.inner.as_ptr())
                .mutex
                .unlock(&mut self.handle.0);
        }
    }
}

/// for convenient usage in where we needs to use the lock standalone
/// 
/// ***For example:***</br>
/// here is some struct
/// ```
/// struct Test {
///     a: u32,
///     b: u64
///     // mutex is only used to protect access of member `b`
///     mutex: Box<FastMutex>
/// }
/// 
/// let test = Box::new(Test{ a: 0, b: 0, FastMutex::create() });
/// // ... then use test across multi threads
/// ```
pub trait MutexNew<T: Mutex> {
    fn create() -> Box<T>;
}

impl<T: Mutex<Target = T>> MutexNew<T> for T {
    fn create() -> Box<T> {
        let mut this = Box::new(unsafe { MaybeUninit::<Self>::zeroed().assume_init() });
        T::init(this.as_mut());
        this
    }
}

pub type GuardLocked<T> = Locked<T, GuardedMutex>;
pub type FastLocked<T> = Locked<T, FastMutex>;
pub type ResouceLocked<T> = Locked<T, ResourceMutex>;
pub type SpinLocked<T> = Locked<T, SpinMutex>;
pub type InStackQueueLocked<T> = StackQueueLocked<T, QueuedSpinMutex>;
