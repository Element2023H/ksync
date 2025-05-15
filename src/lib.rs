#![no_std]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]

pub mod dpc;
pub mod event;
pub mod handle;
pub mod kobject;
pub mod lazy;
pub mod mutex;
pub mod ntstatus;
pub mod once;
pub mod semaphore;
pub mod thread;
pub mod timer;
pub mod utils;
pub mod workitem;

#[deprecated(since = "0.1.1", note = "please use `mutex` instead")]
pub mod lock;

mod constants;
pub(crate) use constants::*;

pub(crate) mod raw;

extern crate alloc;

// call it in another driver to do the testing
pub mod test {
    use core::ptr;
    use core::time::Duration;

    use alloc::sync::Arc;
    use wdk::println;
    use wdk_sys::ntddk::KeGetCurrentProcessorNumberEx;

    use crate::lock;

    use super::mutex::*;
    use super::thread::*;
    use super::event::*;
    use super::dpc::{self};
    use super::semaphore::*;
    use super::kobject::*;
    use super::timer::*;

    extern crate alloc;

    pub fn test_thread() {
        let handle = spawn(|| {
            for i in 0..10 {
                println!("thread {:x} is running", this_thread::id());
                this_thread::sleep(Duration::from_millis(200));
            }
            println!("thread {:x} exited", this_thread::id());
        })
        .unwrap();

        let exit_status = handle.join().expect("join tread failed");

        println!("thread exit status: {:x}", exit_status);
    }

    use alloc::vec::Vec;

    pub fn test_guard_mutex() {
        let mut handles: Vec<JoinHandle> = Vec::new();

        let shared_counter = Arc::new(GuardLocked::new(0u32).unwrap());

        for _ in 0..4 {
            let counter = shared_counter.clone();

            handles.push(
                spawn(move || {
                    for i in 0..100 {
                        if let Ok(mut guard) = counter.lock() {
                            *guard += 1;
                            // this_thread::sleep(Duration::from_millis(i));
                        }
                    }
                })
                .unwrap(),
            );
        }

        // wait for all threads to exit
        for h in handles {
            h.join().expect("join thread failed");
        }

        // check the shared counter
        println!(
            "the final value of shared counter is: {:?}",
            **shared_counter
        );
    }

    pub fn test_fast_mutex() {
        let mut handles: Vec<JoinHandle> = Vec::new();

        let shared_counter = Arc::new(FastLocked::new(0u32).unwrap());

        for _ in 0..4 {
            let counter = shared_counter.clone();

            handles.push(
                spawn(move || {
                    for i in 0..100 {
                        if let Ok(mut guard) = counter.lock() {
                            *guard += 1;
                        }
                    }
                })
                .unwrap(),
            );
        }

        // wait for all threads to exit
        for h in handles {
            h.join().expect("join thread failed");
        }

        // check the shared counter
        println!(
            "the final value of shared counter is: {:?}",
            **shared_counter
        );
    }

    pub fn test_spinlock() {
        let mut handles: Vec<JoinHandle> = Vec::new();

        let shared_counter = Arc::new(SpinLocked::new(0u32).unwrap());

        for _ in 0..4 {
            let counter = shared_counter.clone();

            handles.push(
                spawn(move || {
                    for i in 0..100 {
                        if let Ok(mut guard) = counter.lock() {
                            *guard += 1;
                        }
                    }
                })
                .unwrap(),
            );
        }

        // wait for all threads to exit
        for h in handles {
            h.join().expect("join thread failed");
        }

        // check the shared counter
        println!(
            "the final value of shared counter is: {:?}",
            **shared_counter
        );
    }

    pub fn test_resouce_lock() {
        let mut handles: Vec<JoinHandle> = Vec::new();

        let shared_counter = Arc::new(ResourceLocked::new(0u32).unwrap());

        for _ in 0..4 {
            let counter = shared_counter.clone();

            handles.push(
                spawn(move || {
                    for i in 0..100 {
                        if let Ok(mut guard) = counter.lock() {
                            *guard += 1;
                        }
                    }
                })
                .unwrap(),
            );
        }

        // wait for all threads to exit
        for h in handles {
            h.join().expect("join thread failed");
        }

        // check the shared counter
        println!(
            "the final value of shared counter is: {:?}",
            **shared_counter
        );
    }

    pub fn test_queued_spin_lock() {
        let mut handles: Vec<JoinHandle> = Vec::new();

        let shared_counter = Arc::new(InStackQueueLocked::new(0u32).unwrap());

        for _ in 0..4 {
            let counter = shared_counter.clone();

            handles.push(
                spawn(move || {
                    for _ in 0..1000 {
                        let mut handle = LockedQuueHandle::new();

                        if let Ok(mut guard) = counter.lock(&mut handle) {
                            *guard += 1;
                        }
                    }
                })
                .unwrap(),
            );
        }

        // wait for all threads to exit
        for h in handles {
            h.join().expect("join thread failed");
        }

        // check the shared counter
        println!(
            "the final value of shared counter is: {:?}",
            **shared_counter
        );
    }

    pub fn test_event() {
        // create a auto-reset event(also called SynchronizationEvent)
        let event = Arc::new(
            EventProperty::new()
                .auto_reset(true)
                .initial_state(false)
                .new_event()
                .unwrap(),
        );

        // start main thread
        {
            let event = event.clone();

            let _ = spawn(move || {
                for i in 0..4 {
                    this_thread::sleep(Duration::from_secs(10));
                    event.set();
                }
            });
        }

        // observer thread
        {
            let event = event.clone();
            let _ = spawn(move || {
                if event.wait_for(Duration::from_secs(5), false).timed_out() {
                    println!("wait timed out, thread {} exited", this_thread::id());
                }

                println!("observer thread {} exited", this_thread::id());
            });
        }

        // worker thread
        {
            for _ in 0..4 {
                let event = event.clone();
                let _ = spawn(move || {
                    if event.wait(false).success() {
                        println!("worker thread {} waked up", this_thread::id());
                    }

                    println!("worker thread {} exited", this_thread::id());
                });
            }
        }
    }

    pub fn test_semaphore() {
        let limit = available_parallelism().get();

        let semaphore = Arc::new(Semaphore::new(0, limit as _).unwrap());

        // producer thread
        {
            let repo = semaphore.clone();

            let _ = spawn(move || {
                for _ in 0..4 {
                    this_thread::sleep(Duration::from_secs(5));
                    repo.release(1);
                }

                println!("producer thread {} exited", this_thread::id());
            });
        }

        // consumer thread
        {
            for _ in 0..limit {
                let repo = semaphore.clone();

                let _ = spawn(move || {
                    if repo.wait(false).success() {
                        println!("consumerthread {} wake up", this_thread::id());
                    }

                    println!("consumer thread {} exited", this_thread::id());
                });
            }
        }
    }

    pub fn test_timer() {
        // this timer will use a DPC
        let timer = Arc::new(
            Timer::new(
                || {
                    println!("timer expired");
                },
                false,
            )
            .unwrap(),
        );

        {
            let timer = timer.clone();

            let _ = spawn(move || {
                this_thread::sleep(Duration::from_secs(30));
                timer.stop();

                println!("timer stopped");
            });
        }

        timer.start(Duration::ZERO, Duration::from_secs(5));
    }

    pub fn test_dpc() {
        dpc::run_once_per_core(|| {
            let core = unsafe { KeGetCurrentProcessorNumberEx(ptr::null_mut()) };

            println!("running on core#{}", core);
        });
    }
}
