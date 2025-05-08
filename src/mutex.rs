use crate::ntstatus::NtError;
use core::{
    fmt::{Debug, Display},
    mem::{self},
    ops::{Deref, DerefMut},
    ptr::{self, NonNull, drop_in_place},
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
        KeAcquireInStackQueuedSpinLock, KeAcquireInStackQueuedSpinLockAtDpcLevel,
        KeAcquireSpinLockAtDpcLevel, KeAcquireSpinLockRaiseToDpc, KeGetCurrentIrql,
        KeInitializeEvent, KeInitializeGuardedMutex, KeInitializeSpinLock, KeReleaseGuardedMutex,
        KeReleaseInStackQueuedSpinLock, KeReleaseInStackQueuedSpinLockFromDpcLevel,
        KeReleaseSpinLock, KeReleaseSpinLockFromDpcLevel, KeTryToAcquireGuardedMutex,
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
        unimplemented!("try_lock")
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

/// A Wrapp for kernel Spin lock
///
/// # Safety Warning:
/// be careful when use one spin lock at both HIGH_LEVEL and DISPATCH_LEVEL
/// spin locks are generally not recommended for use at both HIGH_LEVEL and DISPATCH_LEVEL IRQLs.
/// While they can be used at or below DISPATCH_LEVEL, their use at HIGH_LEVEL is limited and can lead to potential issues.
///
/// HIGH_LEVEL is a hardware interrupt while DISPATCH_LEVEL is software interrupt.
/// thread switching is not enabled on IRQL >= DISPATCH_LEVEL.
/// the critical reason for the above is that code running on lower IRQL can be premmited by that running on a higher IRQL
/// and thus may cause deadlocks and data corruption
///
/// as u can see this type permit to used at different IRQL without any constraints, that is ***NOT*** to say u can safely use it concurrently at high IRQL and lower IRQL
/// the design of this interface is only to cover the programmable senmentics provided by Microsoft
///
/// the same rules applied for Queued Spin locks
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
        if unsafe { KeGetCurrentIrql() } == DISPATCH_LEVEL as _ {
            unsafe { KeTryToAcquireSpinLockAtDpcLevel(&mut (*self.inner.as_ptr()).lock) != 0 }
        } else {
            false
        }
    }

    /// a spin lock can be used in IRQL >= DISPATCH_LEVEL and a more efficient function provided by Microsoft
    ///
    /// see https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-keacquirespinlockatdpclevel for details
    fn lock(&self) {
        unsafe {
            let inner = &mut (*self.inner.as_ptr());

            let irql = KeGetCurrentIrql();

            if irql >= DISPATCH_LEVEL as _ {
                KeAcquireSpinLockAtDpcLevel(&mut inner.lock);
            } else {
                inner.irql = KeAcquireSpinLockRaiseToDpc(&mut inner.lock);
            }
        }
    }

    fn unlock(&self) {
        unsafe {
            let inner = &mut (*self.inner.as_ptr());

            let irql = KeGetCurrentIrql();

            if irql >= DISPATCH_LEVEL as _ {
                KeReleaseSpinLockFromDpcLevel(&mut inner.lock);
            } else {
                KeReleaseSpinLock(&mut inner.lock, inner.irql);
            }
        }
    }

    /// a spin lock can safely be held at any IRQL
    fn irql_ok() -> bool {
        true
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

    /// a queued spin lock can be safely held at IRQL >= DISPATCH_LEVEL
    ///
    /// see https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-keacquireinstackqueuedspinlockatdpclevel for details
    fn lock(&self, handle: PKLOCK_QUEUE_HANDLE) {
        let irql = unsafe { KeGetCurrentIrql() };

        if irql >= DISPATCH_LEVEL as _ {
            unsafe {
                KeAcquireInStackQueuedSpinLockAtDpcLevel(self.inner.as_ptr(), handle);
            }
        } else {
            unsafe { KeAcquireInStackQueuedSpinLock(self.inner.as_ptr(), handle) }
        }
    }

    fn unlock(&self, handle: PKLOCK_QUEUE_HANDLE) {
        let irql = unsafe { KeGetCurrentIrql() };

        if irql >= DISPATCH_LEVEL as _ {
            unsafe {
                KeReleaseInStackQueuedSpinLockFromDpcLevel(handle);
            }
        } else {
            unsafe { KeReleaseInStackQueuedSpinLock(handle) };
        }
    }

    /// a queued spin lock can be safely held at any IRQL
    fn irql_ok() -> bool {
        true
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
    mutex: M::Target,
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
/// - shared access, the generic parameter `M` must support shared operations
/// ```
/// let shared_counter = FastLocked::new(0u32).unwrap();
/// if let Ok(counter) = shared_counter.lock_shared() {
///     println!("counter = {}", counter);
/// }
/// ```
///
/// - get immutable reference
/// ```
/// let shared_counter = FastLocked::new(0u32).unwrap();
/// println!("counter = {}", shared_counter.get());
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
            // Rust does not actually "move" the `InnerData` into the memory location where the raw pointer `layout` points to
            // it copy it instead and then call the drop on temporary `InnerData`
            // yes this is a trap here, that's why we use a `ptr::write` to ensure the temporary `InnerData` will
            // not be dropped upon it goes out of scope, since we will drop it manually in `Locked::drop()`
            // The following code is wrong, the temporary `InnerData` will be droppd in place which is not we want
            //*layout = InnerData { ... }
            ptr::write(
                layout,
                InnerData {
                    mutex: M::new(),
                    data,
                },
            );
        };

        Ok(Self {
            inner: NonNull::new(layout).expect("can not allocate memory for Locked<T,M>"),
        })
    }

    pub fn get(&mut self) -> &mut T {
        &mut **self
    }

    /// returns a `MutexGuard` for exclusive access
    ///
    /// the caller can gain a mutable or immutable ref to `T` through `MutexGuard`</br>
    /// the `MutexGuard` implement both `Deref` and `DerefMut` to ensure this
    pub fn lock(&self) -> Result<MutexGuard<'_, true, T, M>, NtError> {
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
    pub fn lock_shared(&self) -> Result<MutexGuard<'_, false, T, M>, NtError> {
        if !M::irql_ok() {
            Err(NtError::from(STATUS_UNSUCCESSFUL))
        } else {
            // this is a wrong usage of a unshareable Mutex, we can not get a `shareable` MutexGuard from a `unshareable` Mutex
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

impl<T, M: Mutex> Deref for Locked<T, M> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &self.inner.as_ref().data }
    }
}

impl<T, M: Mutex> DerefMut for Locked<T, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut self.inner.as_mut().data }
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

/// An RAII implementation of a "scoped lock" of a mutex. When this structure is
/// dropped (falls out of scope), the lock will be unlocked.
///
/// # Parameters
/// - EXCLUSIVE: indicates if the lock should be held exclusive
/// - T: the procted data type
/// - M: the underlying mutex
///
/// # SAFETY
/// the protected `T` can be borrowed as mutable only if the lock can be held exclusively</br>
/// otherwise it is an error and the `DerefMut()` will panic
pub struct MutexGuard<'a, const EXCLUSIVE: bool, T, M: Mutex> {
    locker: &'a Locked<T, M>,
}

impl<'a, const EXCLUSIVE: bool, T, M: Mutex> MutexGuard<'a, EXCLUSIVE, T, M> {
    fn new(locker: &'a Locked<T, M>) -> Self {
        if EXCLUSIVE {
            unsafe { (*locker.inner.as_ptr()).mutex.lock() };
        } else {
            unsafe { (*locker.inner.as_ptr()).mutex.lock_shared() }
        }

        Self { locker }
    }
}

impl<'a, const EXCLUSIVE: bool, T, M: Mutex> Deref for MutexGuard<'a, EXCLUSIVE, T, M> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &self.locker.inner.as_ref().data }
    }
}

impl<'a, const EXCLUSIVE: bool, T, M: Mutex> DerefMut for MutexGuard<'a, EXCLUSIVE, T, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: we can get a mut ref of `T` only when MutexGuard is `locked` exclusively
        // otherwise fail the operation
        if EXCLUSIVE {
            unsafe { &mut (*self.locker.inner.as_ptr()).data }
        } else {
            panic!("can not get a mutable ref of `T` when the lock is not held exclusively");
        }
    }
}

impl<'a, const EXCLUSIVE: bool, T, M: Mutex> Drop for MutexGuard<'a, EXCLUSIVE, T, M> {
    fn drop(&mut self) {
        unsafe {
            if EXCLUSIVE {
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

/// see `SpinMutex` for details
pub struct QueuedSpinMutex {
    inner: NonNull<KSPIN_LOCK>,
}

struct QueuedInnerData<T, M: QueuedMutex> {
    mutex: M::Target,
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
            ptr::write(
                layout,
                QueuedInnerData {
                    mutex: M::new(),
                    data,
                },
            );
        }

        Ok(Self {
            inner: NonNull::new(layout).unwrap(),
        })
    }

    pub fn get(&mut self) -> &mut T {
        &mut **self
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

impl<T, M: QueuedMutex> Deref for StackQueueLocked<T, M> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &self.inner.as_ref().data }
    }
}

impl<T, M: QueuedMutex> DerefMut for StackQueueLocked<T, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut self.inner.as_mut().data }
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

            drop_in_place(&mut self.inner.as_mut().mutex);

            ExFreePoolWithTag(self.inner.as_ptr().cast(), MUTEX_TAG);
        }
    }
}

#[repr(transparent)]
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

unsafe impl<T: Send, M: Mutex> Send for Locked<T, M> {}
unsafe impl<T, M: Mutex> Sync for Locked<T, M> {}

unsafe impl<T: Send, M: QueuedMutex> Send for StackQueueLocked<T, M> {}
unsafe impl<T, M: QueuedMutex> Sync for StackQueueLocked<T, M> {}

pub type GuardLocked<T> = Locked<T, GuardedMutex>;
pub type FastLocked<T> = Locked<T, FastMutex>;
pub type ResouceLocked<T> = Locked<T, ResourceMutex>;
pub type SpinLocked<T> = Locked<T, SpinMutex>;
pub type InStackQueueLocked<T> = StackQueueLocked<T, QueuedSpinMutex>;
