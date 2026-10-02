//! Runtime loading of GPU driver DLLs, so tarsier runs on machines without
//! any particular vendor's driver.

use std::ffi::c_void;

use anyhow::{Context as _, Result};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows::core::{HSTRING, PCSTR};

/// Loads a driver DLL from System32 only. Never freed: function pointers
/// taken from it live for the whole process.
pub fn load_system_library(name: &str) -> Result<HMODULE> {
    unsafe { LoadLibraryExW(&HSTRING::from(name), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
        .with_context(|| format!("{name} not found"))
}

/// Looks up an exported function.
///
/// # Safety
/// `F` must be the function's real signature.
pub unsafe fn export<F: Copy>(lib: HMODULE, name: PCSTR) -> Option<F> {
    let f = unsafe { GetProcAddress(lib, name)? };
    Some(unsafe { fn_ptr(f as *mut c_void) })
}

/// Reinterprets a function address as a typed function pointer.
///
/// # Safety
/// `F` must be the function's real signature.
pub unsafe fn fn_ptr<F: Copy>(address: *mut c_void) -> F {
    const { assert!(size_of::<F>() == size_of::<*mut c_void>()) };
    unsafe { std::mem::transmute_copy(&address) }
}
