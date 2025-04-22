# ksync
rust wrappers for kernel mode thread, FastMutex, GuardMutex, Resources and Queued Spin Locks
# Features
```
// create a joinable system thread(get current thread id) using mod thread
thread::spawn()

// a wrapper for Guarded Mutex
pub type GuardLocked<T> = Locked<T, GuardedMutex>;

// a wrapper for Fast Mutex
pub type FastLocked<T> = Locked<T, FastMutex>;

// a wrapper for ERESOURCE
pub type ResouceLocked<T> = Locked<T, ResourceMutex>;

// a wrapper for Spinlocks
pub type SpinLocked<T> = Locked<T, SpinMutex>;

// a wrapper for Queued Stack Spinlocks
pub type InStackQueueLocked<T> = StackQueueLocked<T, QueuedSpinMutex>;
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
## Example for using Fast/Guarded Mutex
```
use ksync::mutex::*;

let mut handles: Vec<JoinHandle> = Vec::new();

let shared_counter = GuardLocked::new(0u32);

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

let shared_counter = InStackQueueLocked::new(0u32);

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
