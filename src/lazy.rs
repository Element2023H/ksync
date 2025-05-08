use core::{
    cell::UnsafeCell,
    mem::{self, ManuallyDrop},
    ops::Deref,
    ptr::{self, drop_in_place},
    sync::atomic::{self, AtomicU32},
};

union Data<T, F> {
    value: ManuallyDrop<T>,
    f: ManuallyDrop<F>,
}

/// `LazyOnce` is used to initialize `T` exclusively
/// 
/// # Safety
/// - ensure only one thread can only initialize `T` once, other thread must wait until the initializing completed
/// thus no data race occurred during initializing progress
/// - ensure only shared refs can be gained from a `LazyOnce`, so it can be shared between multi-threads
/// 
/// # Example
/// ```
/// // typical usage
/// // declare a `LazyOnce` somewhere
/// static GLOBAL_INSTANCE: lazyONce<0u32> = LazyOnce::new(|| 0);
/// 
/// fn use_global_instance() {
///     println!("value = {}", *GLOBAL_INSTANCE);
/// }
/// 
/// // destroy instance in DriverUnload
/// fn driver_unload(driver_object: PDRIVER_OBJECT) {
///     // the caller must ensure NOT use it again after it dropped
///     lazyOnce::drop(&GLOBAL_INSTANCE);
/// }
/// ```
/// 
/// # Note
/// since only shared refs can be gained through a `LazyOnce`,
/// so if one want to changed the value of `T` inside a `LazyOnce` concurrently,
/// consider wrap `T` within a `Locked` instead.
/// 
/// see `Locked<T>` for details
/// ```
pub struct LazyOnce<T, F = fn() -> T> {
    state: AtomicU32,

    data: UnsafeCell<Data<T, F>>,
}

const UNINIT: u32 = 0;
const INITIALIZING: u32 = 1;
const INITIALIZED: u32 = 2;

impl<T, F: FnOnce() -> T> LazyOnce<T, F> {
    pub const fn new(f: F) -> Self {
        Self {
            state: AtomicU32::new(UNINIT),
            data: UnsafeCell::new(Data {
                f: ManuallyDrop::new(f),
            }),
        }
    }

    /// # Synopsis
    /// use this method to drop `T` inside a `LazyOnce`</br>
    /// `LazyOnce` will not be automatically dropped in kernel programming since kernel leaks something like CRT runtime code</br>
    /// NOR do we can use the `into_inner()` semantics here since rust forbidden move out of static `LazyOnce`
    ///
    /// # Safety
    /// - the caller must call this method at most once
    /// - access the wrapped `T` after dropped can cause undefined behavior
    /// - call `drop()` more than once can cause undefined behavior
    /// - use this method only for the global static initialized `LazyOnce`
    ///
    /// # Examples
    /// ```
    /// // declares LazyOnce in somewhere
    /// static GLOBAL_INSTANCE: LazyOnce<u32> = LazyOnce::new(|| 0);
    ///
    /// void driver_unload(driver_object: PDRIVER_OBJECT) {
    ///     // the inner T will be dropped here
    ///     LazyOnce::drop(&GLOBAL_INSTANCE);
    ///     // ... do some other stuff
    /// }
    /// ```
    pub fn drop(this: &LazyOnce<T, F>) {
        let state = this.state.load(atomic::Ordering::Relaxed);

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

    pub fn force(this: &LazyOnce<T, F>) -> &T {
        let state = this.state.load(atomic::Ordering::Relaxed);

        match state {
            UNINIT => LazyOnce::really_init(this),
            INITIALIZING => this.force_wait(),
            INITIALIZED => unsafe { &(*this.data.get()).value },
            _ => panic!("invalid state value")
        }
    }

    fn really_init(this: &LazyOnce<T, F>) -> &T {
        if let Ok(old_value) = this.state.compare_exchange(
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
                    atomic::Ordering::Release,
                    atomic::Ordering::Relaxed,
                );

                &(*this.data.get()).value
            }
        } else {
            this.force_wait()
        }
    }

    fn force_wait(&self) -> &T {
        self.wait();

        unsafe { &(*self.data.get()).value }   
    }

    // wait until the state becomes State::Initialized
    fn wait(&self) {
        use core::arch::x86_64::_mm_pause;

        while self.state.load(atomic::Ordering::Acquire) != INITIALIZED {
            unsafe { _mm_pause() };
        }
    }
}

impl<T, F: FnOnce() -> T> Deref for LazyOnce<T, F> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        Self::force(self)
    }
}

// `lazyOnce` can be used in many cases not only the global static one
// provide this method for compatible with RAII in rust
impl<T, F> Drop for LazyOnce<T, F> {
    fn drop(&mut self) {
        match *self.state.get_mut() {
            UNINIT => {
                unsafe { ManuallyDrop::drop(&mut self.data.get_mut().f) };
            }
            INITIALIZED => {
                unsafe { ManuallyDrop::drop(&mut self.data.get_mut().value) };
            }
            _ => {}
        }
    }
}

// Safety
// we DO NOT constraint the `T` with `Sync + Send` because some native kernel structs
// contains raw pointers which neither be marked as `Sync` or `Send` by default and thus can not be
// wrapped in LazyOnce which whill fail the compile-time checking
//
// Note
// it is the caller's responsibility to ensure that the raw pointers in `T` can be access safely among multi-thread
//
// unsafe impl<T: Sync + Send, F: Send> Sync for LazyOnce<T, F> {}
unsafe impl<T, F: Send> Sync for LazyOnce<T, F> {}

enum State<T, F> {
    Uninit(F),
    Init(T),
    Poisoned,
}

/// # Synopsis
/// it is used to initialize a `static const` variable that must be initialized at most once but can be used safely in multi-thread</br>
/// this class behave mostly like `LazyOnce` but the users can not get a mutable ref from a `LazyStatic`
/// 
/// # fetures
/// - naturally no data race during initialization(caller must follow the safety rules below)
/// - a `LazyStatic` is memory efficient than a `lazyOnce`, it only require size_of(T) for memory storage
/// 
/// ## Safety
/// - caller must ensure it to be initialized only once, for example: intialize it in DriverEntry by calling `LazyStatic::force`
/// - can be shared between multi-threads
/// - can not obtain a mutable reference through a `LazyStatic` unless using `unsafe` block
/// - no interior mutability once it has been initialized
/// 
/// # Note
/// - `LazyStatic` does not allocate memory in kernel heap, consider using `Box<T>` if `T` must be allocated dynamically
/// - if the caller want to read-write the wrapped `T` concurrently, consider wrap `T` into a `Locked<T>`
/// - once a `LazyStatic` is initialized in static contex(typically a static instance), it is in pinned memory
/// 
/// # Example
/// ```
/// type FnAPI = extern "system" fn ();
/// 
/// pub static KERNEL_API: LazyStatic<Option<FnAPI>> = LazyStatic::new(|| get_kernel_api());
/// 
/// // use it somewhere as follows:
/// fn driver_entry(...) {
///     if let Some(func) = *KERNEL_API {
///         // ...
///     }
/// }
/// ```
pub struct LazyStatic<T, F = fn() -> T> {
    state: UnsafeCell<State<T, F>>,
}

impl<T, F: FnOnce() -> T> LazyStatic<T, F> {
    pub const fn new(f: F) -> Self {
        Self {
            state: UnsafeCell::new(State::Uninit(f)),
        }
    }

    pub fn into_inner(this: LazyStatic<T, F>) -> Result<T, F> {
        match this.state.into_inner() {
            State::Init(data) => Ok(data),
            State::Uninit(f) => Err(f),
            State::Poisoned => panic!("LazyStatic is not initialized")
        }
    }

    pub fn get(&self) -> Option<&T> {
        let state = unsafe { &*self.state.get() };

        match state {
            State::Init(data) => Some(data),
            _ => None
        }
    }

    /// be careful to use this method since it expose a mutable reference to the caller
    /// 
    /// but it is convenient to transfer address of inside `T` to other native fucntions
    pub unsafe fn get_mut(&self) -> Option<&mut T> {
        let state = unsafe { &mut *self.state.get() };

        match state {
            State::Init(data) => Some(data),
            _ => None
        }
    }

    /// Safety:
    /// 
    /// it is the caller's responsibility to call force() in signle-thread mode to avoid data race
    pub fn force(this: &LazyStatic<T, F>) -> &T {
        let state = unsafe { &*this.state.get() };

        match state {
            State::Init(data) => data,
            State::Uninit(_) => unsafe { LazyStatic::really_init(this) },
            State::Poisoned => {
                panic!("LazyStatic is in poisoned state, maybe it has been used incorrectly")
            }
        }
    }

    pub fn drop(this: &LazyStatic<T, F>) {
        let state = unsafe { &mut *this.state.get() };

        match state {
            State::Uninit(_) | State::Init(_) => unsafe {
                drop_in_place(state);
                ptr::write(state, State::Poisoned);
            },
            _ => panic!()
        }
    }
    
    #[cfg(feature = "enable_mut_lazystatic")]
    pub unsafe fn force_mut(this: &LazyStatic<T, F>) -> &mut T {
        let state = unsafe { &mut *this.state.get() };

        match state {
            State::Init(data) => data,
            State::Uninit(_) => unsafe { LazyStatic::really_init_mut(this) },
            State::Poisoned => {
                panic!("LazyStatic is in poisoned state, maybe it has been used incorrectly")
            }
        }
    }

    unsafe fn really_init(this: &LazyStatic<T, F>) -> &T {
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
    unsafe fn really_init_mut(this: &LazyStatic<T, F>) -> &mut T {
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

impl<T, F: FnOnce() -> T> Deref for LazyStatic<T, F> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        LazyStatic::force(self)
    }
}

// about why we do not constraint `Sync + Send` agiant `T`, see ksync::lazy::LazyOnce for details
unsafe impl<T, F: FnOnce() -> T> Sync for LazyStatic<T, F> {}