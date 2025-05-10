//! this module provides OnceCell, OnceLock, LazyCell, LazyLock implementations
//! each has its own respective use cases, and they are different from the std one but acts mostly the same function as it
//!
//! `OnceCell`, `OnceLock` can all be used in where the initialization must be delayed out of object construction
//! while `LazyCell` and `LazyLock` can not
//!
//! - `OnceCell`</br>
//! can be initialized only once in single thread</br>
//! can be safely used in multi-thread by get a shared reference to it</br>
//!
//! - `OnceLock`</br>
//! can be safely initialized only once in multi-thread</br>
//! can be safely used in multi-thread by get a shared reference to it</br>
//!
//! - `LazyCell`</br>
//! can be initialized only once in single thread</br>
//! can be safely used in multi-thread by get a shared reference to it</br>
//!
//! - `lazyLock`</br>
//! can be safely initialized only once in multi-thread</br>
//! can be safely used in multi-thread by get a shared reference to it</br>
//!
//! # Note
//! ***THE REALITY IS:***</br>
//! not all the thread-unsafe code can be detected and avoided in compiler-time checking, especially in kernel programming which heavily depends on the emotion of Microsoft
//!
//! ***THE JOKE IS:***</br>
//! Rust, you can not "enfore" user to write that code that "seems" thread unsafe but actually exactly thread safe
//!
//! the `Cells` an `Lazys` all implement trait `Sync` by default without any constraints to `T`</br>
//! but be careful when use it multi-thread circumstances, since it "SHOULD BE" implemented with `Sync` when `T` is `Sync` under rust language semantics</br>
//!
//! the benefits of that is if we know a sepice of data only need to be initialized once while may contain some raw pointer that returned from kernel API</br>
//! and we excatly sure it can be used safely in multi-thread circumstances without any data race(for example we only read the data but not write or call some </br>
//! API that the thread-safety is already guaranteed by kernel)</br>
//! it will be convenient for us to define and use a static `Cell` or `Lazy` without emiting the compiler check errors</br>
//! think of we have to define a static `Cell` like this:
//! ```
//! // this will emit compiler errors if `OnceCell` has a form of `impl Sync` like this:
//! // "unsafe impl<T: Sync> Sync for OnceCell<T> {}"
//! // SINCE the raw pointer is neither implement `Sync` nor `Send` by default
//! // but we know sometimes we need global data which contains raw pointer to be shared between multi-thread
//! static DEVICE: OnceCell<DRIVER_OBJECT> = OnceCell::new();
//! ```
//!
//! there is a different case that we may want to read/write the global data when using it in multi-thread
//! in this case, we can use `Locked<T>` instead, since this two can ensure data can be access safely in multi-thread circumstances
use core::{
    cell::UnsafeCell,
    mem::{self, ManuallyDrop, MaybeUninit},
    ops::Deref,
    ptr::{self, drop_in_place},
    sync::atomic::{self, AtomicU32, Ordering},
};

const UNINIT: u32 = 0;
const INITIALIZING: u32 = 1;
const INITIALIZED: u32 = 2;

union Data<T, F> {
    value: ManuallyDrop<T>,
    f: ManuallyDrop<F>,
}

/// A value which is initialized on the first access and ensure thread safe during initialization
///
/// # Safety
/// - the value is initialized on its accessed
/// - ensure only one thread can only initialize `T` once, other thread must wait until the initialization completed
/// thus no data race occurred during initialization
/// - ensure only shared refs of `T` can be gained from a `LazyLock` unless get mutable `LazyLock`
///
/// # Example
/// ```
/// // typical usage
/// // declare a `LazyLock` somewhere
/// static GLOBAL_INSTANCE: LazyLock<0u32> = LazyLock::new(|| 0);
///
/// fn use_global_instance() {
///     println!("value = {}", *GLOBAL_INSTANCE);
/// }
///
/// // destroy instance in DriverUnload
/// fn driver_unload(driver_object: PDRIVER_OBJECT) {
///     // the caller must ensure NOT use it again after it dropped
///     LazyLock::drop(&GLOBAL_INSTANCE);
/// }
/// ```
///
/// # Note
/// since only shared refs can be gained through a `LazyLock`,
/// so if one want to changed the value of `T` inside a `LazyLock` concurrently,
/// consider wrap `T` within a `Locked` instead.
///
/// see `Locked<T>` for details
/// ```
pub struct LazyLock<T, F = fn() -> T> {
    state: AtomicU32,

    data: UnsafeCell<Data<T, F>>,
}

impl<T, F: FnOnce() -> T> LazyLock<T, F> {
    pub const fn new(f: F) -> Self {
        Self {
            state: AtomicU32::new(UNINIT),
            data: UnsafeCell::new(Data {
                f: ManuallyDrop::new(f),
            }),
        }
    }

    #[inline]
    fn get_state(&self) -> u32 {
        self.state.load(atomic::Ordering::Relaxed)
    }

    #[inline]
    pub fn get(&self) -> Option<&T> {
        let state = self.get_state();

        match state {
            INITIALIZED => unsafe { Some(&(*self.data.get()).value) },
            _ => None,
        }
    }

    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        let state = self.get_state();

        match state {
            INITIALIZED => unsafe { Some(&mut (*self.data.get()).value) },
            _ => None,
        }
    }

    pub fn force(this: &LazyLock<T, F>) -> &T {
        let state = this.get_state();

        match state {
            UNINIT => LazyLock::really_init(this),
            INITIALIZING => this.force_wait(),
            INITIALIZED => unsafe { &(*this.data.get()).value },
            _ => panic!("invalid state value"),
        }
    }

    fn really_init(this: &LazyLock<T, F>) -> &T {
        if let Ok(_) = this.state.compare_exchange(
            UNINIT,
            INITIALIZING,
            atomic::Ordering::SeqCst,
            atomic::Ordering::Relaxed,
        ) {
            unsafe {
                let data = &mut (*this.data.get());

                let f = ManuallyDrop::take(&mut data.f);

                let value = f();

                (*this.data.get()).value = ManuallyDrop::new(value);

                let _ = this.state.compare_exchange(
                    INITIALIZING,
                    INITIALIZED,
                    atomic::Ordering::SeqCst,
                    atomic::Ordering::Relaxed,
                );

                &(*this.data.get()).value
            }
        } else {
            this.force_wait()
        }
    }

    /// wait until the state becomes State::Initialized and return a valid `&T`
    pub fn force_wait(&self) -> &T {
        self.wait();

        unsafe { &(*self.data.get()).value }
    }

    /// wait until the state becomes State::Initialized
    pub fn wait(&self) {
        use core::arch::x86_64::_mm_pause;

        while self.state.load(atomic::Ordering::Relaxed) != INITIALIZED {
            unsafe { _mm_pause() };
        }
    }

    /// # Synopsis
    /// use this method to drop `T` inside a `LazyLock`</br>
    /// a static `LazyLock` will not be automatically dropped in kernel programming since kernel leaks something like CRT runtime code</br>
    /// NOR do we can use the `into_inner()` semantics here since rust forbidden move out of static `LazyLock`
    ///
    /// # Safety
    /// - the caller must call this method at most once
    /// - access the wrapped `T` after dropped can cause undefined behavior
    /// - call `drop()` more than once can cause undefined behavior
    /// - use this method only for the global static initialized `LazyLock`
    ///
    /// # Examples
    /// ```
    /// // declares LazyLock in somewhere
    /// static GLOBAL_INSTANCE: LazyLock<u32> = LazyLock::new(|| 0);
    ///
    /// void driver_unload(driver_object: PDRIVER_OBJECT) {
    ///     // the inner T will be dropped here
    ///     LazyLock::drop(&GLOBAL_INSTANCE);
    ///     // ... do some other stuff
    /// }
    /// ```
    pub fn drop(this: &LazyLock<T, F>) {
        let state = this.get_state();

        let data = unsafe { &mut *this.data.get() };

        match state {
            UNINIT => unsafe { ManuallyDrop::drop(&mut data.f) },
            INITIALIZED => unsafe { ManuallyDrop::drop(&mut data.value) },
            _ => panic!(),
        }

        // Safety
        // we must ensure all the members be dropped in manually drop operation to prevent memory leaks
        unsafe { drop_in_place(this.state.as_ptr()) };
    }
}

impl<T, F: FnOnce() -> T> Deref for LazyLock<T, F> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        Self::force(self)
    }
}

// deprecated, not the case in driver
// `LazyLock` can be used in many cases not only the global static one
// provide this method for compatible with RAII
// impl<T, F> Drop for LazyLock<T, F> {
//     fn drop(&mut self) {
//         match *self.state.get_mut() {
//             UNINIT => {
//                 unsafe { ManuallyDrop::drop(&mut self.data.get_mut().f) };
//             }
//             INITIALIZED => {
//                 unsafe { ManuallyDrop::drop(&mut self.data.get_mut().value) };
//             }
//             _ => {}
//         }
//     }
// }

// Safety
// we DO NOT constraint the `T` with `Sync + Send` because some native kernel structs
// contains raw pointers which neither be marked as `Sync` or `Send` by default and thus can not be
// wrapped in LazyLock which whill fail the compile-time checking
//
// Note
// it is the caller's responsibility to ensure that the raw pointers in `T` can be access safely among multi-thread
//
// unsafe impl<T: Sync + Send, F: Send> Sync for LazyLock<T, F> {}
// unsafe impl<T, F: Send> Sync for LazyLock<T, F> {}
unsafe impl<T, F: FnOnce() -> T> Sync for LazyLock<T, F> {}

enum State<T, F> {
    Uninit(F),
    Init(T),
    Poisoned,
}

/// # Synopsis
/// A value which is initialized on the first access.
/// it behave mostly like `LazyLock` but is not thread safe during initialization
///
/// # fetures
/// - naturally no data race during initialization(caller must follow the safety rules below)
/// - a `LazyCell` is memory efficient than a `LazyLock`, it only require size_of(T) for memory storage
///
/// ## Safety
/// - caller must ensure it to be initialized only once, for example: intialize it in DriverEntry by calling `LazyCell::force`
/// - can be shared between multi-threads
/// - can not obtain a mutable reference through a `LazyCell` unless using `unsafe` block
/// - no interior mutability once it has been initialized
///
/// # Note
/// - `LazyCell` does not allocate memory in kernel heap, consider using `Box<T>` if `T` must be allocated dynamically
/// - if the caller want to read-write the wrapped `T` concurrently, consider wrap `T` into a `Locked<T>`
/// - once a `LazyCell` is initialized in static contex(typically a static instance), it is in pinned memory
///
/// # Example
/// ```
/// type FnAPI = extern "system" fn ();
///
/// pub static KERNEL_API: LazyCell<Option<FnAPI>> = LazyCell::new(|| get_kernel_api());
///
/// // use it somewhere as follows:
/// fn driver_entry(...) {
///     if let Some(func) = *KERNEL_API {
///         // ...
///     }
/// }
/// ```
pub struct LazyCell<T, F = fn() -> T> {
    state: UnsafeCell<State<T, F>>,
}

impl<T, F: FnOnce() -> T> LazyCell<T, F> {
    pub const fn new(f: F) -> Self {
        Self {
            state: UnsafeCell::new(State::Uninit(f)),
        }
    }

    pub fn into_inner(this: LazyCell<T, F>) -> Result<T, F> {
        match this.state.into_inner() {
            State::Init(data) => Ok(data),
            State::Uninit(f) => Err(f),
            State::Poisoned => panic!("LazyStatic is not initialized"),
        }
    }

    pub fn get(&self) -> Option<&T> {
        let state = unsafe { &*self.state.get() };

        match state {
            State::Init(data) => Some(data),
            _ => None,
        }
    }

    /// be careful to use this method since it expose a mutable reference to the caller
    ///
    /// but it is convenient to transfer address of inside `T` to other native fucntions
    pub fn get_mut(&mut self) -> Option<&mut T> {
        let state = unsafe { &mut *self.state.get() };

        match state {
            State::Init(data) => Some(data),
            _ => None,
        }
    }

    /// Safety:
    ///
    /// it is the caller's responsibility to call force() in signle-thread mode to avoid data race
    pub fn force(this: &LazyCell<T, F>) -> &T {
        let state = unsafe { &*this.state.get() };

        match state {
            State::Init(data) => data,
            State::Uninit(_) => unsafe { LazyCell::really_init(this) },
            State::Poisoned => {
                panic!("LazyStatic is in poisoned state, maybe it has been used incorrectly")
            }
        }
    }

    pub fn drop(this: &LazyCell<T, F>) {
        let state = unsafe { &mut *this.state.get() };

        match state {
            State::Uninit(_) | State::Init(_) => unsafe {
                drop_in_place(state);
                ptr::write(state, State::Poisoned);
            },
            _ => panic!(),
        }
    }

    #[cfg(feature = "enable_mut_lazystatic")]
    pub unsafe fn force_mut(this: &LazyCell<T, F>) -> &mut T {
        let state = unsafe { &mut *this.state.get() };

        match state {
            State::Init(data) => data,
            State::Uninit(_) => unsafe { LazyCell::really_init_mut(this) },
            State::Poisoned => {
                panic!("LazyStatic is in poisoned state, maybe it has been used incorrectly")
            }
        }
    }

    unsafe fn really_init(this: &LazyCell<T, F>) -> &T {
        let state = unsafe { &mut *this.state.get() };

        let State::Uninit(f) = mem::replace(state, State::Poisoned) else {
            unreachable!()
        };

        let data = f();

        unsafe { this.state.get().write(State::Init(data)) };

        let state = unsafe { &*this.state.get() };

        let State::Init(data) = state else {
            unreachable!()
        };

        data
    }

    #[cfg(feature = "enable_mut_lazystatic")]
    unsafe fn really_init_mut(this: &LazyCell<T, F>) -> &mut T {
        let state = unsafe { &mut *this.state.get() };

        let State::Uninit(f) = mem::replace(state, State::Poisoned) else {
            unreachable!()
        };

        let data = f();

        unsafe { this.state.get().write(State::Init(data)) };

        let state = unsafe { &mut *this.state.get() };

        let State::Init(data) = state else {
            unreachable!()
        };

        data
    }
}

impl<T, F: FnOnce() -> T> Deref for LazyCell<T, F> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        LazyCell::force(self)
    }
}

unsafe impl<T, F: FnOnce() -> T> Sync for LazyCell<T, F> {}
// unsafe impl<T, F: FnOnce() -> T> Send for LazyCell<T, F> {}

/// A cell which can nominally be written to only once.
#[repr(transparent)]
pub struct OnceCell<T> {
    inner: UnsafeCell<Option<T>>,
}

impl<T> OnceCell<T> {
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(None),
        }
    }

    #[inline]
    pub fn get(&self) -> Option<&T> {
        unsafe { &*self.inner.get() }.as_ref()
    }

    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        unsafe { &mut *self.inner.get() }.as_mut()
    }

    #[inline]
    pub fn set(&self, value: T) -> Result<(), T> {
        match self.get() {
            Some(_) => Err(value),
            _ => {
                unsafe { *self.inner.get() = Some(value) };

                Ok(())
            }
        }
    }

    #[inline]
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> Option<&T> {
        match self.get() {
            None => {
                let value = f();

                if let Ok(_) = self.set(value) {
                    return self.get();
                }

                None
            }
            _ => return None,
        }
    }

    #[inline]
    pub fn take(&self) -> Option<T> {
        mem::take(unsafe { &mut *self.inner.get() })
    }

    #[inline]
    pub fn into_inner(self) -> Option<T> {
        self.inner.into_inner()
    }

    #[inline]
    pub fn drop(this: &OnceCell<T>) {
        unsafe { *this.inner.get() = None };
    }
}

// Safety
// user must initilize only once, shared between multi-thread through only shared reference
unsafe impl<T> Sync for OnceCell<T> {}

/// A synchronization primitive which can nominally be written to only once.
pub struct OnceLock<T> {
    state: AtomicU32,
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> OnceLock<T> {
    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(UNINIT),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    #[inline]
    pub fn is_initialized(&self) -> bool {
        self.state.load(Ordering::Relaxed) == INITIALIZED
    }

    #[inline]
    pub fn get(&self) -> Option<&T> {
        if self.is_initialized() {
            return Some(unsafe { (&*self.value.get()).assume_init_ref() });
        }

        None
    }

    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        if self.is_initialized() {
            return Some(unsafe { (&mut *self.value.get()).assume_init_mut() });
        }

        None
    }

    /// set a value into underlying data
    ///
    /// initalize underlying `T` with `value` and return Ok(()) if `OnceLock` is not initialized yet
    /// Otherwise return an Err(value)
    #[inline]
    pub fn set(&self, value: T) -> Result<(), T> {
        if let Some(_) = self.get() {
            return Err(value);
        }

        self.init_once(move || value);

        Ok(())
    }

    /// get or initialize the underlying `T`
    ///
    /// return a reference to underlying `T` if it is already initialized, otherwise `None`
    #[inline]
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> Option<&T> {
        if let Some(value) = self.get() {
            return Some(value);
        }

        Some(self.init_once(f))
    }

    /// take the ownership of inside `T`
    ///
    /// # Safety
    /// - the user must not use it again after calling `take`
    /// - use this object again after `take` can cause undefined behavior
    #[inline]
    pub fn take(&mut self) -> Option<T> {
        if self.is_initialized() {
            self.state = AtomicU32::new(UNINIT);

            unsafe { Some((&*self.value.get()).assume_init_read()) }
        } else {
            None
        }
    }

    /// ensure the inside `T` is initialized only once
    fn init_once<F: FnOnce() -> T>(&self, f: F) -> &T {
        if let Ok(_) = self.state.compare_exchange(
            UNINIT,
            INITIALIZING,
            atomic::Ordering::SeqCst,
            atomic::Ordering::Relaxed,
        ) {
            let value = f();

            unsafe { *self.value.get() = MaybeUninit::new(value) };

            // make the inner value aviliable to others theads before they can see `state` changed from `INITIALIZING` or `UNINIT` to `INITIALIZED`
            let _ = self.state.compare_exchange(
                INITIALIZING,
                INITIALIZED,
                atomic::Ordering::SeqCst,
                atomic::Ordering::Relaxed,
            );

            unsafe { (&*self.value.get()).assume_init_ref() }
        } else {
            self.wait()
        }
    }

    /// wait until the state becomes INITIALIZED and return an valid `&T`
    #[inline]
    pub fn wait(&self) -> &T {
        use core::arch::x86_64::_mm_pause;

        while !self.is_initialized() {
            unsafe {
                _mm_pause();
            }
        }

        unsafe { (&*self.value.get()).assume_init_ref() }
    }

    /// associate method that can be used to drop a static `OnceLock` by just hold a immutable reference
    ///
    /// # Safety
    /// - user must call this method only once
    /// - it can never be used after calling `drop` on this object
    ///
    /// # Example
    ///
    /// ```
    /// static GLOBAL_DATA: OnceLock<u32> = OnceLock::new();
    ///
    /// fn driver_entry(driver: DRIVER_OBJECT) {
    ///     GLOBAL_DATA.get_or_init(|| 1);
    /// }
    ///
    /// fn driver_unload(...) {
    ///     // GLOBAL_DATA drops here
    ///     OnceLock::drop(&GLOBAL_DATA);
    ///     // do not use it again
    /// }
    /// ```
    ///
    #[inline]
    pub fn drop(this: &OnceLock<T>) {
        if this.is_initialized() {
            unsafe {
                ptr::drop_in_place(this.state.as_ptr());
                
                // drop the underlying `T`
                (&mut *this.value.get()).assume_init_drop();
            }
        }
    }
}

// impl<T> Drop for OnceLock<T> {
//     fn drop(&mut self) {
//         if self.is_initialized() {
//             unsafe { (&mut *self.value.get()).assume_init_drop() };
//         }
//     }
// }

// unsafe impl<T> Send for OnceLock<T> {}
unsafe impl<T> Sync for OnceLock<T> {}
