#![no_std]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]

pub mod wdm;
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

// just for testing purpose
#[cfg(test)]
pub mod test;

#[deprecated(since = "0.1.1", note = "please use `mutex` instead")]
pub mod lock;

mod constants;
pub(crate) use constants::*;

pub(crate) mod raw;

extern crate alloc;