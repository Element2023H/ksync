# ksync
EN | [中文](./README_CN.md)

rust wrappers for kernel mode Process & Thread, Lazy & Cell, FastMutex, GuardMutex, Resources, Queued Spin Locks, Event, DPC, Semaphore, Timer
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
## Lazy & Once
- each has its own respective use cases, and they are different from the std one but acts mostly functionally the same as it
- `OnceCell`, `OnceLock` can all be used in where the initialization must be delayed out of object construction while `LazyCell` and `LazyLock` can not
- the `OnceXXX` and `LazyXXX` both implement `Sync` by default without any constraints to `T` to make more flexible in most use cases but break a litte rust scoping rules
```
// A value which is initialized on the first access.
lazy::LazyCell
// A value which is initialized on the first access with thread-safe guaranty during initiazation
lazy::LazyLock
// A cell which can nominally be written to only once.
lazy::OnceCell
// A synchronization primitive which can nominally be written to only once.
lazy::OnceLock
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