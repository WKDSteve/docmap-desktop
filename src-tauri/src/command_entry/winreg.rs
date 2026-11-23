//! Windows: the `PATH` of the user in the registry, as much as the command in the terminal needs of it (a string value
//! read with its type, written with one, and the message that the environment changed), through the registry API of the
//! system. No `reg.exe`: a program that starts the tools of the system to change the environment is a pattern an
//! endpoint protection reads as an attack.
//!
//! A trimmed copy of `appkit_hub::winreg` (appkit), as `command_entry.rs` is of `appkit_hub::cli_entry`. Keys are named
//! as `reg.exe` names them (`HKCU\Environment`). Strings are read and written as UTF-16, so a path with any character
//! comes back as it is.
#![allow(unsafe_code)]

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegGetValueW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_EXPAND_SZ,
    REG_SZ, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
};

/// The largest value that is read: a path, or a command line.
const MAX_VALUE_UNITS: usize = 32 * 1024;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The root and the rest of a key named as `reg.exe` names it.
fn split(key: &str) -> Result<(HKEY, Vec<u16>), String> {
    let (root, rest) = key.split_once('\\').unwrap_or((key, ""));
    let root = match root.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => HKEY_CURRENT_USER,
        "HKLM" | "HKEY_LOCAL_MACHINE" => HKEY_LOCAL_MACHINE,
        _ => return Err(format!("not a key of the user or the machine: {key}")),
    };
    Ok((root, wide(rest)))
}

fn os_error(code: u32) -> String {
    std::io::Error::from_raw_os_error(code as i32).to_string()
}

/// Like [`get_typed`], but `None` only when the key or the value is not there. Whoever writes the value back after
/// changing it reads with this: a value that is there and could not be read (no right to it, too large, not a
/// string) is an error, never "nothing there", or the write would replace what it could not see.
///
/// # Errors
///
/// The key is not one of the user or the machine, or the value is there and could not be read.
pub fn get_typed_strict(key: &str, value: &str) -> Result<Option<(String, bool)>, String> {
    match read_value(key, value)? {
        Read::Found(text, expand) => Ok(Some((text, expand))),
        Read::Missing => Ok(None),
        Read::Failed(why) => Err(why),
    }
}

/// What reading a string value came to.
enum Read {
    Found(String, bool),
    /// The key or the value is not there.
    Missing,
    /// It is there and could not be read: why.
    Failed(String),
}

fn read_value(key: &str, value: &str) -> Result<Read, String> {
    let (root, sub) = split(key)?;
    let name = wide(value);
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let mut bytes = 0u32;
    let mut kind = 0u32;
    // SAFETY: a call without a buffer only asks for the size.
    let code = unsafe {
        RegGetValueW(
            root,
            sub.as_ptr(),
            name.as_ptr(),
            flags,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    match code {
        ERROR_SUCCESS => {}
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => return Ok(Read::Missing),
        other => return Ok(Read::Failed(os_error(other))),
    }
    if bytes as usize > MAX_VALUE_UNITS * 2 {
        return Ok(Read::Failed(format!(
            "the value is larger than {MAX_VALUE_UNITS} characters"
        )));
    }
    let mut buf = vec![0u16; (bytes as usize).div_ceil(2) + 1];
    let mut size =
        u32::try_from(buf.len() * 2).map_err(|_| "a value that is too large".to_owned())?;
    // SAFETY: `buf` holds `size` bytes; `kind` receives the type of the value.
    let code = unsafe {
        RegGetValueW(
            root,
            sub.as_ptr(),
            name.as_ptr(),
            flags,
            &mut kind,
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if code != ERROR_SUCCESS {
        return Ok(Read::Failed(os_error(code)));
    }
    let units = (size as usize / 2).min(buf.len());
    let text = &buf[..units];
    let end = text.iter().position(|&u| u == 0).unwrap_or(text.len());
    Ok(Read::Found(
        String::from_utf16_lossy(&text[..end]),
        kind == REG_EXPAND_SZ,
    ))
}

/// Write the string `value` of `key`, as an expandable string (`REG_EXPAND_SZ`) when `expand` is set: a value that
/// holds `%VAR%` and was expandable must stay so, or every `%VAR%` in it stops working.
///
/// # Errors
///
/// What the system said.
pub fn set_typed(key: &str, value: &str, data: &str, expand: bool) -> Result<(), String> {
    let (root, sub) = split(key)?;
    let name = wide(value);
    let text = wide(data);
    let bytes =
        u32::try_from(text.len() * 2).map_err(|_| "a value that is too large".to_owned())?;
    let kind = if expand { REG_EXPAND_SZ } else { REG_SZ };
    // SAFETY: all strings end with a NUL; `text` holds `bytes` bytes.
    let code = unsafe {
        RegSetKeyValueW(
            root,
            sub.as_ptr(),
            name.as_ptr(),
            kind,
            text.as_ptr().cast(),
            bytes,
        )
    };
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(os_error(code))
    }
}

/// Tell the programs of the session that the environment of the user changed (`WM_SETTINGCHANGE` with
/// `"Environment"`), as every installer does after it changed `PATH`: Explorer then gives the new value to the
/// terminals it starts from now on. A window that does not answer within a second is not waited for.
pub fn announce_environment() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    let area = wide("Environment");
    let mut result = 0usize;
    // SAFETY: `area` ends with a NUL and lives until the call returns; the call only reads it.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            area.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            1000,
            &mut result,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The strict read tells a value that is not there from one that is there and cannot be read as a string: the
    /// number `CurrentMajorVersionNumber` (a `REG_DWORD` of every Windows 10 and 11). A writer that took the second
    /// for "nothing there" would replace a whole value (the `PATH` of the user) with one entry.
    #[test]
    fn a_value_that_is_there_but_cannot_be_read_is_an_error_not_nothing() {
        let key = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        assert!(get_typed_strict(key, "CurrentMajorVersionNumber").is_err());
        assert_eq!(get_typed_strict(key, "appkit-no-such-value-6f1c"), Ok(None));
        assert_eq!(
            get_typed_strict(r"HKCU\Software\appkit-test-no-such-key-6f1c", "x"),
            Ok(None)
        );
        let (root, expand) = get_typed_strict(key, "SystemRoot").unwrap().unwrap();
        assert!(root.len() > 3 && !expand);
    }
}
