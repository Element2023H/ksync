//! this mod provide wrappers for c++ like std::unique_lock and std::shared_lock 
use core::{
    mem,
    ptr::{NonNull, drop_in_place},
};

use wdk_sys::{
    _POOL_TYPE::NonPagedPoolNx, STATUS_INSUFFICIENT_RESOURCES, ULONG, ntddk::ExFreePoolWithTag,
};

use crate::{
    mutex::{FastMutex, GuardedMutex, Mutex, ResourceMutex, SpinMutex, ex_allocate_pool_zero},
    ntstatus::NtError,
};

const LOCK_TAG: ULONG = u32::from_ne_bytes(*b"kcol");

/// generic wrapper for standalone locks
pub struct MutexLock<M: Mutex> {
    inner: NonNull<M>,
}

/// describe a locker can be unique
pub trait Uniquable {
    fn lock(&self);
    fn unlock(&self);
}

/// describe a locker can be shared
pub trait Shareable {
    fn lock_shared(&self);
    fn unlock_shared(&self);
}

/// describe a locker can be tryable
pub trait Tryable {
    fn trylock(&self);
}

/// describe a locker can be shared
pub trait TraybleShared {
    fn try_lock_shared(&self);
}

impl<M: Mutex<Target = M>> MutexLock<M> {
    pub fn new() -> Result<Self, NtError> {
        let this =
            ex_allocate_pool_zero(NonPagedPoolNx, mem::size_of::<M>() as _, LOCK_TAG) as *mut M;

        if this.is_null() {
            return Err(STATUS_INSUFFICIENT_RESOURCES.into());
        }

        unsafe { M::init(&mut (*this)) };

        Ok(Self {
            inner: NonNull::new(this).unwrap(),
        })
    }
}

// just forward method to `Mutex`
impl<M: Mutex> Uniquable for MutexLock<M> {
    #[inline(always)]
    fn lock(&self) {
        unsafe { self.inner.as_ref().lock() };
    }

    #[inline(always)]
    fn unlock(&self) {
        unsafe { self.inner.as_ref().unlock() };
    }
}

impl<M: Mutex> Shareable for MutexLock<M> {
    #[inline(always)]
    fn lock_shared(&self) {
        unsafe { self.inner.as_ref().lock_shared() };
    }

    #[inline(always)]
    fn unlock_shared(&self) {
        unsafe { self.inner.as_ref().unlock_shared() };
    }
}

impl<M: Mutex> Drop for MutexLock<M> {
    fn drop(&mut self) {
        unsafe {
            drop_in_place(self.inner.as_ptr());

            ExFreePoolWithTag(self.inner.as_ptr().cast(), LOCK_TAG);
        }
    }
}

pub type FastLock = MutexLock<FastMutex>;
pub type SpinLock = MutexLock<SpinMutex>;
pub type GuardedLock = MutexLock<GuardedMutex>;
pub type ResourceLock = MutexLock<ResourceMutex>;

/// a c++ like unique_lock wrapper for standalone usage
/// # Example
/// ```
/// // define some struct
/// struct Data {
///     a: u8,
///     b: u16,
///     c: u32,
///     d: u64,
///     // this lock is only used to protect member `c` and `d`
///     lock: FastLock
/// }
/// 
/// let data = Data{ a: 0, b: 0, c: 0, d: 0, lock: FastLock::new().unwrap() }
/// 
/// // create a lock guard(using if let statement here is cheap)
/// // the UniqueLock::new() is designed to return Ok() always
/// if let Ok(_) = UniqueLock::new(&data.lock) {
///     data.c += 1;
///     data.d += 1;
/// } // the unique lock is released just after `guard` is out of its scope
/// 
/// ```
pub struct UniqueLock<'a, T: Uniquable> {
    inner: &'a T,
}

impl<'a, T: Uniquable> UniqueLock<'a, T> {
    pub fn new(locker: &'a T) -> Result<Self, ()> {
        locker.lock();
        Ok(Self { inner: locker })
    }
}

impl<T: Uniquable> Drop for UniqueLock<'_, T> {
    fn drop(&mut self) {
        self.inner.unlock();
    }
}

/// a c++ like unique_lock wrapper for standalone usage
/// # Example
/// ```
/// // define some struct
/// struct Data {
///     a: u8,
///     b: u16,
///     c: u32,
///     d: u64,
///     // this lock is only used to protect member `c` and `d`
///     lock: ResourceLock
/// }
/// 
/// let data = Data{ a: 0, b: 0, c: 0, d: 0, lock: ResourceLock::new().unwrap() }
/// 
/// // create a lock guard(using if let statement here is cheap)
/// // the SharedLock::new() is designed to return Ok() always
/// if let Ok(_) = SharedLock::new(&data.lock) {
///     data.c += 1;
///     data.d += 1;
/// } // the unique lock is released just after `guard` is out of its scope
/// 
/// ```
pub struct SharedLock<'a, T: Shareable> {
    inner: &'a T,
}

impl<'a, T: Shareable> SharedLock<'a, T> {
    pub fn new(locker: &'a T) -> Result<Self, ()> {
        locker.lock_shared();
        Ok(Self { inner: locker })
    }
}

impl<T: Shareable> Drop for SharedLock<'_, T> {
    fn drop(&mut self) {
        self.inner.unlock_shared();
    }
}
