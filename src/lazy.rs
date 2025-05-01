use core::{
    cell::UnsafeCell,
    mem::ManuallyDrop,
    ops::Deref,
    sync::atomic::{self, AtomicU32},
};

enum State {
    Uninit,
    Initializing,
    Initialized,
}

union Data<T, F> {
    value: ManuallyDrop<T>,
    f: ManuallyDrop<F>,
}

pub struct LazyOnce<T, F = fn() -> T> {
    state: AtomicU32,

    data: UnsafeCell<Data<T, F>>,
}

impl<T, F: FnOnce() -> T> LazyOnce<T, F> {
    pub const fn new(f: F) -> Self {
        Self {
            state: AtomicU32::new(State::Uninit as u32),
            data: UnsafeCell::new(Data {
                f: ManuallyDrop::new(f),
            }),
        }
    }

    pub fn force(this: &LazyOnce<T, F>) -> &T {
        let state = this.state.load(atomic::Ordering::Relaxed);

        if state == State::Initialized as u32 {
            return unsafe { &(*this.data.get()).value };
        }

        if state == State::Initializing as u32 {
            this.wait();
            return unsafe { &(*this.data.get()).value };
        } else {
            if let Ok(old_value) = this.state.compare_exchange(
                State::Uninit as _,
                State::Initializing as _,
                atomic::Ordering::SeqCst,
                atomic::Ordering::Relaxed,
            ) {
                assert_eq!(old_value, State::Uninit as _);

                unsafe {
                    let data = &mut (*this.data.get());

                    let f = ManuallyDrop::take(&mut data.f);

                    let value = f();

                    (*this.data.get()).value = ManuallyDrop::new(value);

                    let _ = this.state.compare_exchange(
                        State::Initializing as _,
                        State::Initialized as _,
                        atomic::Ordering::Release,
                        atomic::Ordering::Relaxed,
                    );

                    return &(*this.data.get()).value;
                }
            } else {
                this.wait();
                return unsafe { &(*this.data.get()).value };
            }
        }
    }

    // wait until the state becomes State::Initialized
    fn wait(&self) {
        use core::arch::x86_64::_mm_pause;

        while self.state.load(atomic::Ordering::Acquire) != State::Initialized as u32 {
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

const UNINIT: u32 = State::Uninit as u32;
const INITIALIZED: u32 = State::Initialized as u32;

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
unsafe impl<T, F: Send> Sync for LazyOnce<T, F>{}