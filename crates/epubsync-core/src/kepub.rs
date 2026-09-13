//! KEPUB conversion through the kepubify library, which is linked in as a
//! Go static archive built from `kepub-shim/`.

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;

use anyhow::{Context, Result, anyhow};

unsafe extern "C" {
    fn KepubConvert(input: *const c_char, output: *const c_char) -> *mut c_char;
    fn free(ptr: *mut c_void);
}

/// Converts the EPUB at `input` into a KEPUB written to `output`. The output
/// file is created or truncated. On failure the output may hold a partial
/// file, and the caller removes it.
pub fn convert(input: &Path, output: &Path) -> Result<()> {
    let input_c = CString::new(input.as_os_str().as_encoded_bytes())
        .context("input path holds a NUL byte")?;
    let output_c = CString::new(output.as_os_str().as_encoded_bytes())
        .context("output path holds a NUL byte")?;

    // SAFETY: both pointers are valid NUL-terminated strings for the call.
    // The shim returns null or a string allocated with C malloc that we own
    // and free below.
    let err = unsafe { KepubConvert(input_c.as_ptr(), output_c.as_ptr()) };
    if err.is_null() {
        return Ok(());
    }
    let message = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { free(err as *mut c_void) };
    Err(anyhow!("kepubify: {message}"))
}
