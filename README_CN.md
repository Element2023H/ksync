# ksync
中文 | [EN](./README.md)

使用Rust包装Windows内核态的 thread, FastMutex, GuardMutex, Resources and Queued Spin Locks
# 特色
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
## Locked Objects
```
// a wrapper for Guarded Mutex
mutex::GuardLocked<T>

// a wrapper for Fast Mutex
mutex::FastLocked<T>

// a wrapper for ERESOURCE
mutex::ResouceLocked<T>

// a wrapper for Spinlocks
mutex::SpinLocked<T>

// a wrapper for Queued Stack Spinlocks
mutex::InStackQueueLocked<T>
```
## Standalone Locks
```
// a standalone wrapper for Fast Mutex
lock::FastLock

// a standalone wrapper for Spin Lock
lock::SpinLock

// a standalone wrapper for Guarded Mutex
lock::GuardedLock

// a standalone wrapper for ERESOURCE
// note: a ResourceLock is reentrant which means lock_shared & unlock_share is available
lock::ResourceLock

// a c++ STL like wrapper for std::unqiue_lock
lock::UniqueLock

// a c++ STL like wrapper for std::shared_lock
lock::SharedLock
```
# 使用方法
## thread 示例
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
## Fast/Guarded/Resource/Spin mutex locks 示例
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

## Queued Stack Spinlocks 示例
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
## Standalone Locks 示例
```
{
    let mut counter = 0u32;

    // the lock maybe allocated somewhere else
    let lock = lock::FastLock::new().unwrap();

    // create a unique scoped lock
    // we can also use lock.lock() & lock.unlock() inside a code scope
    if let Ok(_) = lock::UniqueLock::new(&lock) {
        counter += 1;
    }

    println!("counter = {}", counter);
}

{
    let mut counter = 0u32;

    // the lock maybe allocated somewhere else
    let lock = lock::ResourceLock::new().unwrap();

    // create a shared scoped lock
    // we can also use lock.lock_shared() & lock.unlock_shared() inside a code scope
    // a resource lock can be both unique and shared
    if let Ok(_) = lock::SharedLock::new(&lock) {
        println!("counter = {}", counter);
    }
}
```