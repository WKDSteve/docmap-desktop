//! Opening a path or an address, and showing a file in its folder, by asking
//! the Windows shell directly.
//!
//! Before this the app started `explorer.exe <target>` and `explorer.exe
//! /select,"<path>"` and let that process do the asking. The two calls here
//! are what Explorer made on the app's behalf; no process of the app's own is
//! started for them. What opens (the browser, the program a file type belongs
//! to) is the user's and outlives the app, as it always did.
//!
//! This is the one module of the app with `unsafe`: the two shell calls and
//! the COM apartment they need have no safe wrapper in the dependency tree.
#![cfg(windows)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use windows_sys::Win32::UI::Shell::Common::ITEMIDLIST;
use windows_sys::Win32::UI::Shell::{
    SHOpenFolderAndSelectItems, SHParseDisplayName, ShellExecuteExW, SEE_MASK_FLAG_NO_UI,
    SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
};

/// `SW_SHOWNORMAL`.
const SHOW_NORMAL: i32 = 1;

/// `text` as a NUL-terminated wide string, or `None` when it holds a NUL
/// itself: the shell would read a shorter target than the one that was checked.
fn wide(text: &str) -> Option<Vec<u16>> {
    if text.contains('\0') {
        return None;
    }
    Some(OsStr::new(text).encode_wide().chain(Some(0)).collect())
}

/// Run `call` on a thread of its own that has a COM apartment.
///
/// The shell wants a single-threaded apartment on the calling thread. The
/// commands of this app run on the main thread or on a pool thread, whose
/// apartment (if any) is not this module's to choose or to change, so the
/// call gets a thread whose apartment begins and ends with it.
fn in_apartment<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let worker = std::thread::Builder::new()
        .name("shell-open".into())
        .spawn(move || {
            // SAFETY: the reserved pointer is documented as null; the flags
            // are the two the shell documentation names for ShellExecuteEx.
            let init = unsafe {
                CoInitializeEx(
                    std::ptr::null(),
                    (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
                )
            };
            if init < 0 {
                return Err(format!("COM could not be initialised ({init:#010x})"));
            }
            let result = call();
            // SAFETY: balances the successful CoInitializeEx above, on the
            // same thread, after everything that used the apartment is done.
            unsafe { CoUninitialize() };
            result
        })
        .map_err(|e| e.to_string())?;
    worker
        .join()
        .map_err(|_| "the shell call panicked".to_string())?
}

/// Hand `target` (a folder, a document or an `http(s)` address) to the shell,
/// which opens it with what the user has set for it.
///
/// The target is one string, never a command line: nothing in it is parsed
/// for arguments, quotes or operators. A failure comes back as text and is
/// not shown by the shell in a dialog of its own (`SEE_MASK_FLAG_NO_UI`).
pub(crate) fn open(target: &str) -> Result<(), String> {
    let file = wide(target).ok_or("the target holds a NUL character")?;
    in_apartment(move || {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            // NOASYNC: the thread ends right after the call, so the shell
            // must be done with it by then.
            fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
            lpFile: file.as_ptr(),
            nShow: SHOW_NORMAL,
            ..Default::default()
        };
        // SAFETY: `info` is a fully initialised SHELLEXECUTEINFOW of the
        // stated size; `file` is NUL-terminated and outlives the call; every
        // other pointer is null, which the call documents as "not given".
        // No handle is asked for (no SEE_MASK_NOCLOSEPROCESS), so there is
        // nothing to close afterwards.
        let ok = unsafe { ShellExecuteExW(&mut info) };
        if ok != 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().to_string())
        }
    })
}

/// Show the file or folder at `path` selected in a window of its folder.
///
/// `path` is a native absolute path (backslashes, no `\\?\` prefix: the shell
/// does not parse one).
pub(crate) fn reveal(path: &str) -> Result<(), String> {
    let name = wide(path).ok_or("the path holds a NUL character")?;
    in_apartment(move || {
        let mut item: *mut ITEMIDLIST = std::ptr::null_mut();
        // SAFETY: `name` is NUL-terminated and outlives the call; `item` is a
        // valid place for the one pointer the call writes; the bind context
        // and the attribute out-pointer are optional and null.
        let parsed = unsafe {
            SHParseDisplayName(
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut item,
                0,
                std::ptr::null_mut(),
            )
        };
        if parsed < 0 || item.is_null() {
            return Err(std::io::Error::from_raw_os_error(parsed).to_string());
        }
        // SAFETY: `item` is the list SHParseDisplayName just allocated. With
        // no child items the call selects `item` itself in its parent folder.
        let shown = unsafe { SHOpenFolderAndSelectItems(item, 0, std::ptr::null(), 0) };
        // SAFETY: `item` was allocated by the shell with the COM allocator and
        // is not used after this line.
        unsafe { CoTaskMemFree(item.cast()) };
        if shown < 0 {
            return Err(std::io::Error::from_raw_os_error(shown).to_string());
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn missing(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("docmap-shellopen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join(name).to_string_lossy().into_owned()
    }

    /// What is not there is an error the app can show. `explorer <missing>`
    /// started fine and opened the Documents folder instead; and the shell
    /// shows no dialog of its own here, or this test would never return.
    #[test]
    fn opening_what_is_not_there_is_an_error_not_another_folder() {
        let err = open(&missing("nothing here.txt")).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn revealing_what_is_not_there_is_an_error() {
        let err = reveal(&missing("nothing here.txt")).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn a_nul_in_the_target_is_refused_before_the_shell_sees_it() {
        assert!(open("C:\\Windows\0\\evil").is_err());
        assert!(reveal("C:\\Windows\0\\evil").is_err());
    }

    /// The hand check: opens the temp folder, then shows a file selected in
    /// it. Windows appear, so it does not run with the suite:
    /// `cargo test hand_check_open_and_reveal -- --ignored --nocapture`.
    #[test]
    #[ignore = "opens two Explorer windows"]
    fn hand_check_open_and_reveal() {
        let dir = std::env::temp_dir().join("docmap-shellopen-hand-check");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("select me & see.txt");
        std::fs::write(&file, b"x").unwrap();
        open(&dir.to_string_lossy()).unwrap();
        reveal(&file.to_string_lossy()).unwrap();
        println!("pid {}", std::process::id());
        // Long enough to look at the process list for a child of this test.
        std::thread::sleep(std::time::Duration::from_secs(8));
    }
}
