use anyhow::{Context, Result};
use std::path::Path;

#[cfg(windows)]
fn quote(value: &str) -> String {
    // Windows CommandLineToArgvW/CRT escaping, including a trailing backslash before the closing quote.
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        if c == '"' {
            result.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
        } else {
            result.extend(std::iter::repeat_n('\\', slashes));
        }
        result.push(c);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}

#[cfg(windows)]
pub fn background(executable: &Path, args: &[String]) -> Result<()> {
    use std::{
        mem::{size_of, zeroed},
        os::windows::ffi::OsStrExt,
        ptr::null,
    };
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS, PROCESS_INFORMATION,
            STARTF_USESHOWWINDOW, STARTUPINFOW,
        },
        UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE,
    };
    let app: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let line = std::iter::once(executable.to_string_lossy().to_string())
        .chain(args.iter().cloned())
        .map(|part| quote(&part))
        .collect::<Vec<_>>()
        .join(" ");
    let mut command: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
    // SAFETY: all pointers reference initialized structs or NUL-terminated UTF-16 buffers which
    // remain live for CreateProcessW. Process/thread handles are closed immediately after creation.
    unsafe {
        let mut startup: STARTUPINFOW = zeroed();
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        startup.dwFlags = STARTF_USESHOWWINDOW;
        startup.wShowWindow = SW_SHOWMINNOACTIVE as u16;
        let mut process: PROCESS_INFORMATION = zeroed();
        if CreateProcessW(
            app.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            0,
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
            null(),
            null(),
            &startup,
            &mut process,
        ) == 0
        {
            return Err(std::io::Error::last_os_error()).context("Cannot start background Chrome");
        }
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
    }
    // No foreground-window activation or simulated focus changes.
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn background(executable: &Path, args: &[String]) -> Result<()> {
    use std::process::{Command, Stdio};
    let app = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .context("Chrome executable must be inside a .app bundle on macOS")?;
    let status = Command::new("/usr/bin/open")
        .args(["-g", "-n", "-a"])
        .arg(app)
        .arg("--args")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    anyhow::ensure!(
        status.success(),
        "macOS could not open Chrome in the background"
    );
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn background(executable: &Path, args: &[String]) -> Result<()> {
    use std::process::{Command, Stdio};
    Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Cannot start Chrome")?;
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn quotes_windows_paths_without_losing_trailing_slashes() {
        assert_eq!(quote(r"C:\my profile\"), "\"C:\\my profile\\\\\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
    }
}
