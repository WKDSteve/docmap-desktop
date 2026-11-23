//! Which other instances of this program run, and closing their windows: what `docmap-desktop status` and `stop` need.
//!
//! The app has no hub and no server to ask, so a running window is found the way the system knows it: the processes
//! whose program is this very file (the same path, not a build of another folder). Nothing is started and no process
//! is ended: `stop` posts `WM_CLOSE` to the visible windows of those processes, which is what their close button does,
//! so a window that asks something (an unsaved agent run) is still asked and can stay. A program is looked at through
//! the list of the system and by the path of its image, with the right to query, never to read its memory.
//!
//! Windows only: elsewhere the app is not built to be closed from outside, and the two commands say so.
//!
//! This is one of the modules of the app with `unsafe` (`shellopen.rs` and `command_entry/winreg.rs` are the others).

#[cfg(windows)]
pub(crate) use imp::{close_windows, others};

#[cfg(not(windows))]
pub(crate) fn others(_own: &std::path::Path) -> Result<Vec<u32>, String> {
    Err("status and stop are built for Windows only".to_owned())
}

#[cfg(not(windows))]
pub(crate) fn close_windows(_pids: &[u32]) -> usize {
    0
}

#[cfg(windows)]
mod imp {
    #![allow(unsafe_code)]

    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, HWND, INVALID_HANDLE_VALUE, LPARAM,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    };

    /// A path as the system writes it, compared the way Windows compares: without the verbatim prefix, without case,
    /// with one kind of separator.
    fn plain(path: &Path) -> String {
        path.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .to_ascii_lowercase()
    }

    /// The program file of the process `pid`; none when it may not be asked (another user's, a protected one).
    fn image_of(pid: u32) -> Option<PathBuf> {
        // SAFETY: the handle is closed below; the buffer holds `size` UTF-16 units and the call says how many it used.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buffer = vec![0u16; 32 * 1024];
            let mut size = u32::try_from(buffer.len()).ok()?;
            let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
            CloseHandle(handle);
            (ok != 0).then(|| PathBuf::from(OsString::from_wide(&buffer[..size as usize])))
        }
    }

    /// The ids of the processes other than this one that run the program file `own`.
    pub(crate) fn others(own: &Path) -> Result<Vec<u32>, String> {
        let name = own
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .ok_or_else(|| "the program has no file name".to_owned())?;
        let want = plain(own);
        let me = std::process::id();
        let mut found = Vec::new();
        // SAFETY: `entry.dwSize` is set as the API asks, the snapshot is closed below, and the name is read only up to
        // its end in the fixed array of the entry.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(format!(
                    "the list of processes cannot be read (error {})",
                    GetLastError()
                ));
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = u32::try_from(std::mem::size_of::<PROCESSENTRY32W>())
                .map_err(|_| "a process entry too large".to_owned())?;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe = String::from_utf16_lossy(&entry.szExeFile[..end]).to_ascii_lowercase();
                let pid = entry.th32ProcessID;
                if exe == name
                    && pid != me
                    && image_of(pid).is_some_and(|image| plain(&image) == want)
                {
                    found.push(pid);
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
        Ok(found)
    }

    struct Closing<'a> {
        pids: &'a [u32],
        posted: usize,
    }

    unsafe extern "system" fn each_window(window: HWND, lparam: LPARAM) -> i32 {
        // SAFETY: `lparam` is the address of the `Closing` that `close_windows` keeps alive for the whole enumeration.
        let closing = unsafe { &mut *(lparam as *mut Closing<'_>) };
        let mut pid = 0u32;
        // SAFETY: `window` is a handle the system gave to this callback; `pid` is a valid place to write to.
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        // SAFETY: as above; a message to a window that is gone meanwhile is refused by the system, not undefined.
        let visible = unsafe { IsWindowVisible(window) } != 0;
        if visible
            && closing.pids.contains(&pid)
            && unsafe { PostMessageW(window, WM_CLOSE, 0, 0) } != 0
        {
            closing.posted += 1;
        }
        1
    }

    /// Ask the visible windows of the processes `pids` to close, as their close button does. Returns how many windows
    /// were asked.
    pub(crate) fn close_windows(pids: &[u32]) -> usize {
        let mut closing = Closing { pids, posted: 0 };
        // SAFETY: the callback only runs inside this call, while `closing` is alive.
        unsafe {
            EnumWindows(Some(each_window), std::ptr::addr_of_mut!(closing) as LPARAM);
        }
        closing.posted
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_path_is_compared_as_windows_does() {
            assert_eq!(
                plain(Path::new(r"\\?\C:\Apps\Docmap-Desktop.exe")),
                plain(Path::new("c:/apps/docmap-desktop.exe"))
            );
            assert_ne!(
                plain(Path::new(r"C:\Apps\docmap-desktop.exe")),
                plain(Path::new(r"C:\Other\docmap-desktop.exe"))
            );
        }

        /// Another process that runs a program file is found by that file, and this process never finds itself.
        #[test]
        fn another_process_of_the_same_program_is_found_and_this_one_is_not() {
            let system = std::env::var_os("SystemRoot").expect("SystemRoot");
            let cmd = Path::new(&system).join("System32").join("cmd.exe");
            let mut child = std::process::Command::new(&cmd)
                .args(["/d", "/c", "ping -n 30 127.0.0.1 >nul"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .spawn()
                .expect("cmd starts");
            let found = others(&cmd).unwrap();
            let mine = std::process::id();
            let seen = found.contains(&child.id());
            let myself = found.contains(&mine);
            let nothing_to_close = close_windows(&[child.id()]);
            let _ = child.kill();
            let _ = child.wait();
            assert!(seen, "{found:?} does not hold the child {}", child.id());
            assert!(!myself);
            // A console process has no visible window of its own to close.
            assert_eq!(nothing_to_close, 0);
            // A program that no process runs has no other instance.
            let none = Path::new(&system)
                .join("System32")
                .join("no-such-program-6f1c.exe");
            assert_eq!(others(&none).unwrap(), Vec::<u32>::new());
        }
    }
}
