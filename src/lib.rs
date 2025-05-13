#![no_std]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]

pub mod utils;
pub mod mutex;
pub mod once;
pub mod lazy;
pub mod ntstatus;
pub mod thread;

#[deprecated(since="0.1.1", note="please use `mutex` instead")]
pub mod lock;

mod constants;
pub(crate) use constants::*;

extern crate alloc;

// call it in another driver to do the testing
pub mod test {
    use core::time::Duration;

    use alloc::sync::Arc;
    use wdk::println;

    use crate::lock;

    use super::mutex::*;
    use super::thread::*;

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
        println!("the final value of shared counter is: {:?}", **shared_counter);
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
        println!("the final value of shared counter is: {:?}", **shared_counter);
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
        println!("the final value of shared counter is: {:?}", **shared_counter);
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
        println!("the final value of shared counter is: {:?}", **shared_counter);
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
        println!("the final value of shared counter is: {:?}", **shared_counter);
    }

}
