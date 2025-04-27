use crate::ntstatus::NtError;
use core::{
    fmt::{Debug, Display},
    mem::{self, ManuallyDrop},
    ops::{Deref, DerefMut},
    ptr::{NonNull, drop_in_place},
};
use wdk_sys::{
    _EVENT_TYPE::SynchronizationEvent,
    _POOL_TYPE::NonPagedPoolNx,
    APC_LEVEL, DISPATCH_LEVEL, ERESOURCE, FALSE, FAST_MUTEX, FM_LOCK_BIT, KGUARDED_MUTEX, KIRQL,
    KLOCK_QUEUE_HANDLE, KSPIN_LOCK, PKLOCK_QUEUE_HANDLE, PVOID, SIZE_T,
    STATUS_INSUFFICIENT_RESOURCES, STATUS_SUCCESS, STATUS_UNSUCCESSFUL, TRUE, ULONG,
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
    use wdk_sys::POOL_TYPE;

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

pub use otf::ex_allocate_pool_zero;

const MUTEX_TAG: ULONG = u32::from_ne_bytes(*b"xetm");

pub trait Mutex {
    type Target: Mutex;

    fn new() -> Self::Target;

    fn shareable() -> bool {
        false
    }

    fn lock(&self);

    fn try_lock(&self) -> bool {
        unimplemented!("trylock")
    }

    fn lock_shared(&self) {
        unimplemented!("lock_shared")
    }

    fn try_lock_shared(&self) -> bool {
        unimplemented!("try_lock_shared")
    }

    fn unlock_shared(&self) {
        unimplemented!("unlock_shared")
    }

    fn unlock(&self);

    fn irql_ok() -> bool {
        return unsafe { KeGetCurrentIrql() <= APC_LEVEL as u8 };
    }
}

pub trait QueuedMutex {
    type Target: QueuedMutex;

    fn new() -> Self::Target;

    fn lock(&self, handle: PKLOCK_QUEUE_HANDLE);

    fn unlock(&self, handle: PKLOCK_QUEUE_HANDLE);

    fn irql_ok() -> bool {
        return unsafe { KeGetCurrentIrql() <= DISPATCH_LEVEL as u8 };
    }
}

pub struct EmptyMutex;

pub struct FastMutex {
    inner: NonNull<FAST_MUTEX>,
}

pub struct GuardedMutex {
    inner: NonNull<KGUARDED_MUTEX>,
}

pub struct ResourceMutex {
    inner: NonNull<ERESOURCE>,
}

pub struct SpinMutex {
    inner: NonNull<SpinLockInner>,
}

unsafe impl Send for EmptyMutex {}
unsafe impl Send for FastMutex {}
unsafe impl Send for GuardedMutex {}
unsafe impl Send for ResourceMutex {}
unsafe impl Send for SpinMutex {}

impl Mutex for EmptyMutex {
    type Target = Self;

    fn new() -> Self::Target {
        Self
    }

    fn lock(&self) {}

    fn unlock(&self) {}
}

impl Mutex for FastMutex {
    type Target = Self;

    fn new() -> Self::Target {
        let mutex =
            ex_allocate_pool_zero(NonPagedPoolNx, mem::size_of::<FAST_MUTEX>() as _, MUTEX_TAG)
                as *mut FAST_MUTEX;

        if !mutex.is_null() {
            ExInitializeFastMutex(mutex);
        }

        Self {
            inner: NonNull::new(mutex).expect("can not allocate memory for FastMutex"),
        }
    }

    fn try_lock(&self) -> bool {
        unsafe { ExTryToAcquireFastMutex(self.inner.as_ptr()) != 0 }
    }

    fn lock(&self) {
        unsafe {
            ExAcquireFastMutex(self.inner.as_ptr());
        }
    }

    fn unlock(&self) {
        unsafe { ExReleaseFastMutex(self.inner.as_ptr()) };
    }
}

impl Drop for FastMutex {
    fn drop(&mut self) {
        unsafe {
            ExFreePoolWithTag(self.inner.as_ptr().cast(), MUTEX_TAG);
        }
    }
}

impl Mutex for GuardedMutex {
    type Target = Self;

    fn new() -> Self::Target {
        let mutex = ex_allocate_pool_zero(
            NonPagedPoolNx,
            mem::size_of::<KGUARDED_MUTEX>() as _,
            MUTEX_TAG,
        ) as *mut KGUARDED_MUTEX;

        if !mutex.is_null() {
            unsafe { KeInitializeGuardedMutex(mutex) };
        }

        Self {
            inner: NonNull::new(mutex).expect("can not allocate memory for Guarded Mutex"),
        }
    }

    fn try_lock(&self) -> bool {
        unsafe { KeTryToAcquireGuardedMutex(self.inner.as_ptr()) != 0 }
    }

    fn lock(&self) {
        unsafe {
            KeAcquireGuardedMutex(self.inner.as_ptr());
        }
    }

    fn unlock(&self) {
        unsafe { KeReleaseGuardedMutex(self.inner.as_ptr()) };
    }
}

impl Drop for GuardedMutex {
    fn drop(&mut self) {
        unsafe {
            ExFreePoolWithTag(self.inner.as_ptr().cast(), MUTEX_TAG);
        }
    }
}

impl Mutex for ResourceMutex {
    type Target = Self;

    fn new() -> Self::Target {
        let mutex =
            ex_allocate_pool_zero(NonPagedPoolNx, mem::size_of::<ERESOURCE>() as _, MUTEX_TAG)
                as *mut ERESOURCE;

        if !mutex.is_null() {
            match unsafe { ExInitializeResourceLite(mutex) } {
                STATUS_SUCCESS => (),
                _ => panic!("can not initialize ERESOURCE"),
            }
        }

        Self {
            inner: NonNull::new(mutex).expect("can not allocate memory for ERESOURCE"),
        }
    }

    fn shareable() -> bool {
        true
    }

    fn try_lock(&self) -> bool {
        unsafe { ExAcquireResourceExclusiveLite(self.inner.as_ptr(), FALSE as _) != 0 }
    }

    fn lock(&self) {
        unsafe {
            ExAcquireResourceExclusiveLite(self.inner.as_ptr(), TRUE as _);
        }
    }

    fn unlock(&self) {
        unsafe { ExReleaseResourceLite(self.inner.as_ptr()) };
    }

    fn try_lock_shared(&self) -> bool {
        unsafe { ExAcquireResourceSharedLite(self.inner.as_ptr(), FALSE as _) != 0 }
    }

    fn lock_shared(&self) {
        unsafe {
            ExAcquireResourceSharedLite(self.inner.as_ptr(), TRUE as _);
        }
    }

    fn unlock_shared(&self) {
        unsafe {
            ExReleaseResourceLite(self.inner.as_ptr());
        }
    }
}

impl Drop for ResourceMutex {
    fn drop(&mut self) {
        unsafe {
            let _ = ExDeleteResourceLite(self.inner.as_ptr());
            ExFreePoolWithTag(self.inner.as_ptr().cast(), MUTEX_TAG);
        }
    }
}

struct SpinLockInner {
    irql: KIRQL,
    lock: KSPIN_LOCK,
}

impl Mutex for SpinMutex {
    type Target = Self;

    fn new() -> Self::Target {
        let mutex = ex_allocate_pool_zero(
            NonPagedPoolNx,
            mem::size_of::<SpinLockInner>() as _,
            MUTEX_TAG,
        ) as *mut SpinLockInner;

        if !mutex.is_null() {
            unsafe {
                (*mutex).irql = 0;
                KeInitializeSpinLock(&mut (*mutex).lock);
            }
        }

        Self {
            inner: NonNull::new(mutex).expect("can not allocated memory for KSPIN_LOCK"),
        }
    }

    fn try_lock(&self) -> bool {
        unsafe { KeTryToAcquireSpinLockAtDpcLevel(&mut (*self.inner.as_ptr()).lock) != 0 }
    }

    fn lock(&self) {
        unsafe {
            let inner = &mut (*self.inner.as_ptr());

            inner.irql = KeAcquireSpinLockRaiseToDpc(&mut inner.lock);
        }
    }

    fn unlock(&self) {
        unsafe {
            KeReleaseSpinLock(
                &mut (*self.inner.as_ptr()).lock,
                (*self.inner.as_ptr()).irql,
            );
        }
    }

    // KeAcquireSpinLock can only be called at IRQL <= DISPATCH_LEVEL
    fn irql_ok() -> bool {
        unsafe { KeGetCurrentIrql() <= DISPATCH_LEVEL as u8 }
    }
}

impl Drop for SpinMutex {
    fn drop(&mut self) {
        unsafe { ExFreePoolWithTag(self.inner.as_ptr().cast(), MUTEX_TAG) };
    }
}

impl QueuedMutex for QueuedSpinMutex {
    type Target = Self;

    fn new() -> Self::Target {
        let mutex =
            ex_allocate_pool_zero(NonPagedPoolNx, mem::size_of::<KSPIN_LOCK>() as _, MUTEX_TAG)
                as *mut KSPIN_LOCK;

        if !mutex.is_null() {
            unsafe {
                KeInitializeSpinLock(mutex);
            }
        }

        Self {
            inner: NonNull::new(mutex).expect("can not allocated memory for QueuedSpinMutex"),
        }
    }

    fn lock(&self, handle: PKLOCK_QUEUE_HANDLE) {
        unsafe { KeAcquireInStackQueuedSpinLock(self.inner.as_ptr(), handle) }
    }

    fn unlock(&self, handle: PKLOCK_QUEUE_HANDLE) {
        unsafe {
            KeReleaseInStackQueuedSpinLock(handle);
        }
    }
}

impl Drop for QueuedSpinMutex {
    fn drop(&mut self) {
        unsafe { ExFreePoolWithTag(self.inner.as_ptr().cast(), MUTEX_TAG) };
    }
}

/// the internal layout for `Locked<T,M>`
///
/// this has the same layout as `QueuedInnerData`
struct InnerData<T, M: Mutex> {
    /// using `ManuallyDrop` here to ensure safety</br>
    /// we must ensure memory consistency in `Mutex` which lives as long as Locked<T, M></br>
    /// it should not be dropped upon it goes out of scope of `Locked::new()`
    mutex: ManuallyDrop<M::Target>,
    data: T,
}

/// a strategy lock wrapper for FastMutex, GuardMutex, Spinlock, Resources
///
/// it is used combined with FastMutex, GuardedMutex, SpinMutex, and ResourceMutex types
///
/// # Example
/// - unique access
/// ```
/// let shared_counter = FastLocked::new(0u32).unwrap();
/// if let Ok(mut counter) = shared_counter.lock {
///     *counter += 1;
/// }
/// ```
/// - shared access
/// ```
/// let shared_counter = FastLocked::new(0u32).unwrap();
/// if let Ok(counter) = shared_counter.lock_shared() {
///     println!("counter = {}", counter);
/// }
/// ```
pub struct Locked<T, M>
where
    M: Mutex,
{
    inner: NonNull<InnerData<T, M>>,
}

impl<T, M: Mutex> Locked<T, M> {
    pub fn new(data: T) -> Result<Self, NtError> {
        let layout = ex_allocate_pool_zero(
            NonPagedPoolNx,
            mem::size_of::<InnerData<T, M>>() as _,
            MUTEX_TAG,
        ) as *mut InnerData<T, M>;

        if layout.is_null() {
            return Err(STATUS_INSUFFICIENT_RESOURCES.into());
        }

        unsafe {
            // rust does not actually "move" the `InnerData` into the memory location where the raw pointer `layout` points to
            // yes this is a trap here(in fact, it just memcpy it rather than move), that's why we use a `ManuallyDrop` to ensure the heap allocated `InnerData` will
            // not be dropped upon it goes out of scope, since we will drop it manually in `Locked::drop()`
            *layout = InnerData {
                mutex: ManuallyDrop::new(M::new()),
                data,
            };
        };

        Ok(Self {
            inner: NonNull::new(layout).expect("can not allocate memory for Locked<T,M>"),
        })
    }

    /// returns a `MutexGuard` for exclusive access
    ///
    /// the caller can gain a mutable or immutable ref to `T` through `MutexGuard`</br>
    /// the `MutexGuard` implement both `Deref` and `DerefMut` to ensure this
    pub fn lock(&self) -> Result<MutexGuard<'_, T, M>, NtError> {
        if !M::irql_ok() {
            Err(NtError::from(STATUS_UNSUCCESSFUL))
        } else {
            Ok(MutexGuard::new(self))
        }
    }

    /// returns a `MutexGuard` for shared access
    ///
    /// the caller can only gain a immutable ref of `T` through `MutexGuard`
    ///
    /// ***NOTE***:
    /// 
    /// maybe we need a some type like `SharedMutexGuard` that only implements `Deref`?
    /// but i think using compile-time constant here is a good choice
    pub fn lock_shared(&self) -> Result<MutexGuard<'_, T, M>, NtError> {
        if !M::irql_ok() {
            Err(NtError::from(STATUS_UNSUCCESSFUL))
        } else {
            // this is a wrong usage of a unshareable Mutex
            // the result of M::shareable() will be optmized as compile-time constant, so it is zero-cost
            if !M::shareable() {
                #[cfg(debug_assertions)]
                panic!("Can not call lock_shared on a unshareable Mutex");

                Err(NtError::from(STATUS_UNSUCCESSFUL))
            } else {
                Ok(MutexGuard { locker: self })
            }
        }
    }
}

impl<T, M: Mutex> Drop for Locked<T, M> {
    fn drop(&mut self) {
        unsafe {
            drop_in_place(&mut self.inner.as_mut().data);

            ManuallyDrop::drop(&mut self.inner.as_mut().mutex);

            ExFreePoolWithTag(self.inner.as_ptr().cast(), MUTEX_TAG);
        }
    }
}

impl<T: Display, M: Mutex> Debug for Locked<T, M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Locked{{{}}}", unsafe { &(*self.inner.as_ptr()).data })
    }
}

/// An RAII implementation of a "scoped lock" of a mutex. When this structure is
/// dropped (falls out of scope), the lock will be unlocked.
pub struct MutexGuard<'a, T, M: Mutex> {
    locker: &'a Locked<T, M>,
}

impl<'a, T, M: Mutex> MutexGuard<'a, T, M> {
    fn new(locker: &'a Locked<T, M>) -> Self {
        if !M::shareable() {
            unsafe { (*locker.inner.as_ptr()).mutex.lock() };
        } else {
            unsafe { (*locker.inner.as_ptr()).mutex.lock_shared() }
        }

        Self { locker }
    }
}

impl<'a, T, M: Mutex> Deref for MutexGuard<'a, T, M> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &self.locker.inner.as_ref().data }
    }
}

impl<'a, T, M: Mutex> DerefMut for MutexGuard<'a, T, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut (*self.locker.inner.as_ptr()).data }
    }
}

impl<'a, T, M: Mutex> Drop for MutexGuard<'a, T, M> {
    fn drop(&mut self) {
        unsafe {
            if !M::shareable() {
                (*self.locker.inner.as_ptr()).mutex.unlock();
            } else {
                (*self.locker.inner.as_ptr()).mutex.unlock_shared();
            }
        }
    }
}

pub struct QueuedEmptyMutex;

impl QueuedMutex for QueuedEmptyMutex {
    type Target = Self;

    fn new() -> Self::Target {
        Self
    }

    fn lock(&self, handle: PKLOCK_QUEUE_HANDLE) {
        let _ = handle;
    }

    fn unlock(&self, handle: PKLOCK_QUEUE_HANDLE) {
        let _ = handle;
    }
}

pub struct QueuedSpinMutex {
    inner: NonNull<KSPIN_LOCK>,
}

struct QueuedInnerData<T, M: QueuedMutex> {
    mutex: ManuallyDrop<M::Target>,
    data: T,
}

/// a strategy lock wrapper for Queued Spin Locks
///
/// a Queued Spin Lock is a special spin lock that can improve system performance, see
/// https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/queued-spin-locks for details
///
/// # Example
/// ```
/// let mut handle = LockedQuueHandle::new();
/// if let Ok(mut counter) = shared_counter.lock(&mut handle) {
///     *counter += 1;
/// }
/// ```
pub struct StackQueueLocked<T, M: QueuedMutex> {
    inner: NonNull<QueuedInnerData<T, M>>,
}

impl<T, M: QueuedMutex> StackQueueLocked<T, M> {
    pub fn new(data: T) -> Result<Self, NtError> {
        let layout = ex_allocate_pool_zero(
            NonPagedPoolNx,
            mem::size_of::<QueuedInnerData<T, M>>() as _,
            MUTEX_TAG,
        ) as *mut QueuedInnerData<T, M>;

        if layout.is_null() {
            return Err(STATUS_INSUFFICIENT_RESOURCES.into());
        }

        unsafe {
            *layout = QueuedInnerData {
                mutex: ManuallyDrop::new(M::new()),
                data,
            }
        }

        Ok(Self {
            inner: NonNull::new(layout).unwrap(),
        })
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
                locker: self,
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

impl<T, M: QueuedMutex> Drop for StackQueueLocked<T, M> {
    fn drop(&mut self) {
        unsafe {
            drop_in_place(&mut (*self.inner.as_ptr()).data);

            ManuallyDrop::drop(&mut self.inner.as_mut().mutex);

            ExFreePoolWithTag(self.inner.as_ptr().cast(), MUTEX_TAG);
        }
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
    locker: &'a StackQueueLocked<T, M>,
}

impl<'a, T, M: QueuedMutex> Deref for InStackMutexGuard<'a, T, M> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &self.locker.inner.as_ref().data }
    }
}

impl<'a, T, M: QueuedMutex> DerefMut for InStackMutexGuard<'a, T, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut (*self.locker.inner.as_ptr()).data }
    }
}

impl<'a, T, M: QueuedMutex> Drop for InStackMutexGuard<'a, T, M> {
    fn drop(&mut self) {
        unsafe {
            (*self.locker.inner.as_ptr())
                .mutex
                .unlock(&mut self.handle.0);
        }
    }
}

unsafe impl<T, M: Mutex> Send for Locked<T, M> {}
unsafe impl<T, M: Mutex> Sync for Locked<T, M> {}

unsafe impl<T, M: QueuedMutex> Send for StackQueueLocked<T, M> {}
unsafe impl<T, M: QueuedMutex> Sync for StackQueueLocked<T, M> {}

pub type GuardLocked<T> = Locked<T, GuardedMutex>;
pub type FastLocked<T> = Locked<T, FastMutex>;
pub type ResouceLocked<T> = Locked<T, ResourceMutex>;
pub type SpinLocked<T> = Locked<T, SpinMutex>;
pub type InStackQueueLocked<T> = StackQueueLocked<T, QueuedSpinMutex>;
