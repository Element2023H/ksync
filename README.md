# ksync
EN | [中文](./README_CN.md)

rust wrappers for kernel mode thread, FastMutex, GuardMutex, Resources and Queued Spin Locks
# Features
## Threads
```
// create a joinable system thread
thread::spawn()

// sleeping
thread::this_thread::sleep()

// get thread id
thread::this_thread::id()

// yielding
thread::this_thread::pause()
```
## Mutexs
```
// a wrapper for Fast Mutex
mutex::FastMutex

// a wrapper for Guarded Mutex
mutex::GuardedMutex

// a wrapper for ERESOURCE
mutex::ResourceMutex

// a wrapper for Spinlock
mutex::SpinMutex
```
## Locks
```
// Lock T with mutex::GuardMutex
mutex::GuardLocked<T>

// Lock T with mutex::FastMutex
mutex::FastLocked<T>

// Lock T with mutex::ResourceMutex
mutex::ResouceLocked<T>

// Lock T with mutex::SpinMutex
mutex::SpinLocked<T>

// Lock T with mutex::QueuedSpinMutex
mutex::InStackQueueLocked<T>
```
# Usage
## Example for using thread
```
use ksync::thread::{self, this_thread};

let mut handle = thread::spawn(|| {
    for i in 0..10 {
        println!("thread {:x} is running", this_thread::id());
        this_thread::sleep(Duration::from_millis(200));
    }
    println!("thread {:x} exited", this_thread::id());
})
.unwrap();

handle.join().expect("join tread failed");

println!("thread exit status: {:x}", handle.exit_status().unwrap());
```
## Example for using Fast/Guarded/Resource/Spin mutex locks
```
use ksync::mutex::*;

let mut handles: Vec<JoinHandle> = Vec::new();

// For Fast Mutex:
// let let shared_counter = FastLocked::new(0u32).unwrap();
// For Resources:
// let shared_counter = ResourceLocked::new(0u32).unwrap();
// For Spinlocks:
// let shared_counter = SpinLocked::new(0u32).unwrap();
let shared_counter = GuardLocked::new(0u32).unwrap();

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
```

## Example for using Queued Stack Spinlocks
```
use ksync::mutex::*;

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

```