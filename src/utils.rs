#[macro_export]
macro_rules! handle_to_ulong {
    ($a:expr) => {
        $a as wdk_sys::ULONG
    };
}

#[macro_export]
macro_rules! ulong_to_handle {
    ($a:expr) => {
        $a as wdk_sys::HANDLE
    };
}

#[macro_export]
macro_rules! initialize_object_attributes {
    () => {
        wdk_sys::OBJECT_ATTRIBUTES {
            Length: core::mem::size_of::<wdk_sys::OBJECT_ATTRIBUTES>() as _,
            ..unsafe { mem::zeroed }
        }
    };
    ($n:expr, $a:expr, $r:expr, $s:expr) => {
        wdk_sys::OBJECT_ATTRIBUTES {
            Length: core::mem::size_of::<wdk_sys::OBJECT_ATTRIBUTES>() as _,
            RootDirectory: $r,
            Attributes: $a,
            ObjectName: $n,
            SecurityDescriptor: $s,
            SecurityQualityOfService: ptr::null_mut(),
        }
    };
}
