//! Protects saved passwords with Windows DPAPI (bound to the current Windows user).

#[repr(C)]
struct DataBlob {
    cb_data: u32,
    pb_data: *mut u8,
}

#[cfg(windows)]
#[link(name = "crypt32")]
extern "system" {
    fn CryptProtectData(input: *const DataBlob, descr: *const u16, entropy: *const DataBlob, reserved: *const std::ffi::c_void, prompt: *const std::ffi::c_void, flags: u32, output: *mut DataBlob) -> i32;
    fn CryptUnprotectData(input: *const DataBlob, descr: *mut *mut u16, entropy: *const DataBlob, reserved: *const std::ffi::c_void, prompt: *const std::ffi::c_void, flags: u32, output: *mut DataBlob) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(mem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
}

const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

/// Returns the encrypted text as hex, or an empty string when encryption is unavailable.
pub fn protect(plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    #[cfg(windows)]
    unsafe {
        let mut bytes = plain.as_bytes().to_vec();
        let input = DataBlob { cb_data: bytes.len() as u32, pb_data: bytes.as_mut_ptr() };
        let mut out = DataBlob { cb_data: 0, pb_data: std::ptr::null_mut() };
        if CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) == 0 {
            return String::new();
        }
        let enc = std::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec();
        LocalFree(out.pb_data as *mut _);
        return enc.iter().map(|b| format!("{b:02x}")).collect();
    }
    #[cfg(not(windows))]
    String::new()
}

/// Reverses `protect`; returns an empty string for bad input or another Windows user.
pub fn unprotect(hex: &str) -> String {
    if hex.is_empty() || hex.len() % 2 != 0 {
        return String::new();
    }
    let Some(mut bytes) = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()).collect::<Option<Vec<u8>>>() else {
        return String::new();
    };
    #[cfg(windows)]
    unsafe {
        let input = DataBlob { cb_data: bytes.len() as u32, pb_data: bytes.as_mut_ptr() };
        let mut out = DataBlob { cb_data: 0, pb_data: std::ptr::null_mut() };
        if CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) == 0 {
            return String::new();
        }
        let plain = std::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec();
        LocalFree(out.pb_data as *mut _);
        return String::from_utf8_lossy(&plain).into_owned();
    }
    #[cfg(not(windows))]
    {
        let _ = &mut bytes;
        String::new()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn protect_then_unprotect_returns_original_text() {
        let enc = protect("s3cret-ä");
        assert!(!enc.is_empty());
        assert!(!enc.contains("s3cret"));
        assert_eq!(unprotect(&enc), "s3cret-ä");
    }

    #[test]
    fn unprotect_rejects_garbage() {
        assert_eq!(unprotect("zz"), "");
        assert_eq!(unprotect("abc"), "");
    }
}
