# ksync
EN | [中文](./README_CN.md)

rust wrappers for kernel mode Process & Thread, Lazy & Cell, FastMutex, GuardMutex, Resources, Queued Spin Locks, Event, DPC, Semaphore, Timer
# Features
## Kernel Object
Owned kernel object encapsulation: `KernelObject<T>` , it will manage the object reference automatically(use RAII) with well defined error handling</br>
- Get a process object from PID
```
if let Ok(process) = ProcessObject::from_process_id(0x1024) {
    // ...
} // process will be dropped here
```
- Get a thread object from a raw HANDLE:
```
if let Ok(thread) = ThreadObject::from_thread_handle(0x1024 as HANDLE) {
    // ...
} // thread will be dropped here
```
- Dispatchable trait

since this trait is implement for `KernelObject<T>`  so any kernel object that support dispatch, just like Thread, Event, Mutex, Semaphore, Timer etc will benefits from it</br>
this is an example to test if a thread is exited
```
fn is_thread_exited(h_thread: HANDLE) -> Result<bool, NtError> {
    match ThreadObject::from_thread_handle(0x1024 as HANDLE) {
        Ok(thread) => Ok(thread.wait_for(Duration::ZERO, false).success()),
        Err(e) => Err(e)
    }
}
```
- ObjectHandle

a `ObjectHandle` represent a HANDLE that created from some kernel object, this will increse the reference count of a kernel object, for all kernel object, this pattern applies</br>
here is an example create a `ObjectHandle` from a raw kernel process object
```
if let Ok(handle) = ObjectHandle::from_process(process, GENERIC_ALL) {
    // we successfully got a handle to that process object
    // ... do something
} // handle will be dropped here
```
`KernelObject<T>` and ObjectHandle can be transformed from each other, see the source code for details

## Thread Operations
```
// create a joinable system thread
thread::spawn()

// sleeping
thread::this_thread::sleep()

// get thread id
thread::this_thread::id()

// yielding
thread::this_thread::pause()

// available parallelism
thread::available_parallelism()
```

## Lazy & Once
- they Each has its own respective use cases, and they are different from the `std one` but acts mostly functionally the same as it
- `OnceCell`, `OnceLock` can all be used in where the initialization must be delayed out of object construction scope while `LazyCell` and `LazyLock` can not
- the `OnceXXX` and `LazyXXX` both implement `Sync` if `T` is `Sync` by default to gain more flexibility in most use cases
- you will need it when u have some global data pieces that may have different initializing strategies
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
## Locked
convenient wrappers to use FastMutex, GuardMutex, Resources, SpinLocks and QueuedSpinLocks used in many cases
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
## Event, Semaphore, DPC and Timer
- Event and Semaphore are basic kernel synchronization primitives that can be easily used
- DPCs are different, they are actually piece of code that can be scheduled for execution at some time</br>
there is a simple example for using DPC that only execute once, the closure will be executed on each CPU core no more than once:

```
dpc::run_once_per_core(|| {
    let core = unsafe { KeGetCurrentProcessorNumberEx(ptr::null_mut()) };

    println!("running on core#{}", core);
});
```

if u want a owned DPC type, see `Dpc` and `ThreadedDpc` for details, in fact, a Timer also use a `Dpc` inside as backend

- Timer is also designed as a owned type that can be used periodically, the following example demonstrate how to use a period timer
```
// this timer will use a DPC as backend
let timer = Arc::new(
    Timer::new(
        || {
            println!("timer expired");
        },
        false,
    )
    .unwrap(),
);

timer.start(Duration::ZERO, Duration::from_secs(5));
```
BTW, High Resolution Timer is on the way...

# Example For Locks
## using Locks
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

## Using Queued SpinLocks
```
use ksync::mutex::*;

let mut handles: Vec<JoinHandle> = Vec::new();

let shared_counter = Arc::new(InStackQueueLocked::new(0u32).unwrap());

for _ in 0..4 {
    let counter = shared_counter.clone();

    handles.push(
        spawn(|| {
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
println!("the final value of shared counter is: {:?}", *shared_counter);

```