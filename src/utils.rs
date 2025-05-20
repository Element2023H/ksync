use alloc::{boxed::Box, vec::Vec};
use core::{alloc::Layout, arch::asm, mem, ptr};
use wdk_sys::{
    _POOL_TYPE::PagedPool, PIO_STACK_LOCATION, PIRP, PKTHREAD, POOL_TYPE, PUNICODE_STRING, PVOID,
    SIZE_T, SL_PENDING_RETURNED, ULONG, ULONG_PTR, UNICODE_STRING, WCHAR, ntddk::ExFreePoolWithTag,
};

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
#[inline(always)]
pub(crate) fn read_gs_qword(offset: u64) -> u64 {
    let value: u64;
    unsafe {
        asm!(
            "mov {}, gs:[{}]",
            out(reg) value,
            in(reg) offset,
        );
    }
    value
}

#[allow(non_snake_case)]
pub(crate) fn KeGetCurrentThread() -> PKTHREAD {
    (read_gs_qword(0x188) as PVOID).cast()
}

pub(crate) const fn ctl_code(dev_type: u32, function: u32, method: u32, access: u32) -> u32 {
    (dev_type << 16) | ((access) << 14) | ((function) << 2) | (method)
}

#[allow(non_snake_case)]
pub(crate) fn IoGetCurrentIrpStackLocation(irp: PIRP) -> PIO_STACK_LOCATION {
    if !(unsafe { (*irp).CurrentLocation <= (*irp).StackCount }) {
        panic!();
    }

    unsafe {
        (*irp)
            .Tail
            .Overlay
            .__bindgen_anon_2
            .__bindgen_anon_1
            .CurrentStackLocation
    }
}

#[allow(non_snake_case)]
pub(crate) fn IoMarkIrpPending(irp: PIRP) {
    (unsafe { *IoGetCurrentIrpStackLocation(irp) }).Control |= SL_PENDING_RETURNED as u8;
}

pub(crate) fn utf16_from_str(s: &str) -> Option<Box<UNICODE_STRING>> {
    unicode_from_str(s).map(|buffer| unsafe { Box::from_raw(buffer as *mut UNICODE_STRING) })
}

unsafe extern "C" {
    pub fn ExAllocatePoolWithTag(pool_type: POOL_TYPE, size: SIZE_T, tag: ULONG) -> PVOID;
}

pub(crate) fn ex_allocate_pool_zero(pool_type: POOL_TYPE, size: SIZE_T, tag: ULONG) -> PVOID {
    let ptr = unsafe { ExAllocatePoolWithTag(pool_type, size, tag) };

    if !ptr.is_null() {
        unsafe { ptr::write_bytes(ptr, 0, size as _) };
    }

    ptr
}

/// stable rust forbids to use a customized allocator with Box<T> like this:
///
/// type PagedBox<T> = alloc::boxed::Box<T, PagedAllocator>;
pub(crate) struct PagedAllocator;

const RUST_PAGED_TAG: ULONG = u32::from_ne_bytes(*b"egap");

impl PagedAllocator {
    pub fn allocate(&self, layout: core::alloc::Layout) -> *mut u8 {
        let ptr = ex_allocate_pool_zero(PagedPool, layout.size() as u64, RUST_PAGED_TAG);

        if ptr == ptr::null_mut() {
            return ptr::null_mut();
        }

        ptr.cast()
    }

    pub fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        unsafe { ExFreePoolWithTag(ptr.cast(), RUST_PAGED_TAG) };
    }
}

pub(crate) fn unicode_from_str(s: &str) -> Option<PUNICODE_STRING> {
    let value: Vec<_> = s.encode_utf16().collect();

    let al = PagedAllocator;

    let char_size = value.len() * mem::size_of::<WCHAR>();

    let buffer = al.allocate(
        Layout::from_size_align(
            char_size + mem::size_of::<UNICODE_STRING>(),
            mem::size_of::<ULONG_PTR>(),
        )
        .unwrap(),
    );

    if !buffer.is_null() {
        let header = unsafe { (buffer as *mut UNICODE_STRING).as_mut().unwrap() };

        // assign header
        header.Length = char_size as u16;
        header.MaximumLength = char_size as u16;
        header.Buffer = (buffer as *mut UNICODE_STRING).wrapping_offset(1).cast();

        // copy string
        unsafe {
            buffer
                .wrapping_offset(mem::size_of::<UNICODE_STRING>() as _)
                .copy_from_nonoverlapping(value.as_ptr().cast(), char_size)
        };

        return Some(buffer as _);
    }

    None
}
