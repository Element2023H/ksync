#![no_std]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]

pub mod lock;
pub mod utils;
pub mod mutex;
pub mod lazy;
pub mod ntstatus;
pub mod thread;

mod constants;
pub(crate) use constants::*;

extern crate alloc;

// call it in another driver to do the testing
pub mod test {
    use core::time::Duration;

    use wdk::println;

    use crate::lock;

    use super::mutex::*;
    use super::thread::*;

    extern crate alloc;

    pub fn test_thread() {
        let mut handle = spawn(|| {
            for i in 0..10 {
                println!("thread {:x} is running", this_thread::id());
                this_thread::sleep(Duration::from_millis(200));
            }
            println!("thread {:x} exited", this_thread::id());
        })
        .unwrap();

        handle.join().expect("join tread failed");

        println!("thread exit status: {:x}", handle.exit_status().unwrap());
    }

    use alloc::vec::Vec;

    pub fn test_guard_mutex() {
        let mut handles: Vec<JoinHandle> = Vec::new();

        let shared_counter = GuardLocked::new(0u32).unwrap();

        for _ in 0..4 {
            handles.push(
                spawn(|| {
                    for i in 0..100 {
                        if let Ok(mut counter) = shared_counter.lock() {
                            *counter += 1;
                            // this_thread::sleep(Duration::from_millis(i));
                        }
                    }
                })
                .unwrap(),
            );
        }

        // wait for all threads to exit
        for mut h in handles {
            h.join().expect("join thread failed");
        }

        // check the shared counter
        println!("the final value of shared counter is: {:?}", shared_counter);
    }

    pub fn test_fast_mutex() {
        let mut handles: Vec<JoinHandle> = Vec::new();

        let shared_counter = FastLocked::new(0u32).unwrap();

        for _ in 0..4 {
            handles.push(
                spawn(|| {
                    for i in 0..100 {
                        if let Ok(mut counter) = shared_counter.lock() {
                            *counter += 1;
                        }
                    }
                })
                .unwrap(),
            );
        }

        // wait for all threads to exit
        for mut h in handles {
            h.join().expect("join thread failed");
        }

        // check the shared counter
        println!("the final value of shared counter is: {:?}", shared_counter);
    }

    pub fn test_spinlock() {
        let mut handles: Vec<JoinHandle> = Vec::new();

        let shared_counter = SpinLocked::new(0u32).unwrap();

        for _ in 0..4 {
            handles.push(
                spawn(|| {
                    for i in 0..100 {
                        if let Ok(mut counter) = shared_counter.lock() {
                            *counter += 1;
                        }
                    }
                })
                .unwrap(),
            );
        }

        // wait for all threads to exit
        for mut h in handles {
            h.join().expect("join thread failed");
        }

        // check the shared counter
        println!("the final value of shared counter is: {:?}", shared_counter);
    }

    pub fn test_resouce_lock() {
        let mut handles: Vec<JoinHandle> = Vec::new();

        let shared_counter = ResouceLocked::new(0u32).unwrap();

        for _ in 0..4 {
            handles.push(
                spawn(|| {
                    for i in 0..100 {
                        if let Ok(mut counter) = shared_counter.lock() {
                            *counter += 1;
                        }
                    }
                })
                .unwrap(),
            );
        }

        // wait for all threads to exit
        for mut h in handles {
            h.join().expect("join thread failed");
        }

        // check the shared counter
        println!("the final value of shared counter is: {:?}", shared_counter);
    }

    pub fn test_queued_spin_lock() {
        let mut handles: Vec<JoinHandle> = Vec::new();

        let shared_counter = InStackQueueLocked::new(0u32).unwrap();

        for _ in 0..4 {
            handles.push(
                spawn(|| {
                    for _ in 0..1000 {
                        let mut handle = LockedQuueHandle::new();

                        if let Ok(mut counter) = shared_counter.lock(&mut handle) {
                            *counter += 1;
                        }
                    }
                })
                .unwrap(),
            );
        }

        // wait for all threads to exit
        for mut h in handles {
            h.join().expect("join thread failed");
        }

        // check the shared counter
        println!("the final value of shared counter is: {:?}", shared_counter);
    }

    pub fn test_standalone_locks() {
        let mut counter = 0u32;

        let lock = FastMutex::new();

        if let Ok(_) = lock::UniqueLock::new(&lock) {
            counter += 1;
        }

        println!("counter = {}", counter);
    }
}
