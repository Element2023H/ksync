use wdk_sys::HANDLE;

pub const NtCurrentProcess: HANDLE = u64::MAX as HANDLE;
pub const ZwCurrentProcess: HANDLE = NtCurrentProcess;
pub const NtCurrentThread: HANDLE = (u64::MAX - 1) as HANDLE;
pub const ZwCurrentThread: HANDLE = NtCurrentThread;
pub const NtCurrentSession: HANDLE = (u64::MAX - 2) as HANDLE;
pub const ZwCurrentSession: HANDLE = NtCurrentSession;
