use anyhow::{Context, Result};
use std::path::Path;

#[cfg(any(target_os = "macos", test))]
fn wait_launcher(
    mut child: std::process::Child,
    timeout: std::time::Duration,
) -> Result<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Background Chrome launcher timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

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
    let child = Command::new("/usr/bin/open")
        .args(["-g", "-n", "-a"])
        .arg(app)
        .arg("--args")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Cannot start macOS Chrome launcher")?;
    // LaunchServices can stall without exposing a CDP endpoint. Never block the worker indefinitely.
    let status = wait_launcher(child, std::time::Duration::from_secs(15))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn quotes_windows_paths_without_losing_trailing_slashes() {
        assert_eq!(quote(r"C:\my profile\"), "\"C:\\my profile\\\\\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn launcher_helper_process() {
        if std::env::var_os("RESEARCH_TEST_LAUNCHER_STALL").is_some() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }

    #[test]
    fn launcher_wait_is_bounded_and_reaps_the_child() -> Result<()> {
        let helper = |stall| -> Result<std::process::Child> {
            let mut command = std::process::Command::new(std::env::current_exe()?);
            command.args(["--exact", "launch::tests::launcher_helper_process"]);
            command.stdout(std::process::Stdio::null());
            command.stderr(std::process::Stdio::null());
            if stall {
                command.env("RESEARCH_TEST_LAUNCHER_STALL", "1");
            }
            Ok(command.spawn()?)
        };
        assert!(wait_launcher(helper(false)?, std::time::Duration::from_secs(5))?.success());
        let started = std::time::Instant::now();
        let error = wait_launcher(helper(true)?, std::time::Duration::from_millis(100))
            .expect_err("stalled launcher must fail");
        assert!(error.to_string().contains("launcher timed out"));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        Ok(())
    }
}
