use anyhow::{Context, Result};
use std::path::Path;

#[cfg(not(windows))]
pub fn spawn(dir: &Path, executable: &Path) -> Result<std::process::Child> {
    use std::process::{Command, Stdio};
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("service.log"))?;
    let mut command = Command::new(executable);
    command
        .arg("serve")
        .arg("--home")
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .spawn()
        .context("Cannot start background research daemon")
}

#[cfg(windows)]
pub use windows::{daemon_stderr, spawn};

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        mem::{size_of, zeroed},
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        ptr::null,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::{
            Console::{STD_ERROR_HANDLE, SetStdHandle},
            Threading::{
                CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS, GetExitCodeProcess,
                PROCESS_INFORMATION, STARTUPINFOW, WaitForSingleObject,
            },
        },
    };

    pub struct Child(OwnedHandle);
    impl Child {
        pub fn try_wait(&mut self) -> Result<Option<u32>> {
            // SAFETY: the owned process handle remains live for these read-only calls.
            unsafe {
                match WaitForSingleObject(self.0.as_raw_handle(), 0) {
                    WAIT_TIMEOUT => Ok(None),
                    WAIT_OBJECT_0 => {
                        let mut code = 0;
                        if GetExitCodeProcess(self.0.as_raw_handle(), &mut code) == 0 {
                            return Err(std::io::Error::last_os_error().into());
                        }
                        Ok(Some(code))
                    }
                    _ => Err(std::io::Error::last_os_error().into()),
                }
            }
        }
    }

    fn quote(value: &str) -> String {
        let mut result = String::from("\"");
        let mut slashes = 0;
        for c in value.chars() {
            if c == '\\' {
                slashes += 1;
                continue;
            }
            result.extend(std::iter::repeat_n(
                '\\',
                if c == '"' { slashes * 2 + 1 } else { slashes },
            ));
            result.push(c);
            slashes = 0;
        }
        result.extend(std::iter::repeat_n('\\', slashes * 2));
        result.push('"');
        result
    }

    pub fn spawn(dir: &Path, executable: &Path) -> Result<Child> {
        let app: Vec<u16> = executable
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let line = format!(
            "{} serve --home {}",
            quote(
                executable
                    .to_str()
                    .context("Research executable path is not representable as Unicode")?
            ),
            quote(
                dir.to_str()
                    .context("Research data home is not representable as Unicode")?
            )
        );
        let mut command: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
        // No inherited handles: std::process::Command can keep the caller's capture
        // pipes open even after connect/activate exits. The daemon opens its own log.
        // SAFETY: all buffers/structs remain live through CreateProcessW; returned
        // handles transfer to OwnedHandle or close immediately.
        unsafe {
            let mut startup: STARTUPINFOW = zeroed();
            startup.cb = size_of::<STARTUPINFOW>() as u32;
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
                return Err(std::io::Error::last_os_error())
                    .context("Cannot start background research daemon");
            }
            CloseHandle(process.hThread);
            Ok(Child(OwnedHandle::from_raw_handle(process.hProcess)))
        }
    }

    pub fn daemon_stderr(dir: &Path) -> Result<std::fs::File> {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("service.log"))?;
        // SAFETY: the caller retains the File for the complete daemon lifetime.
        if unsafe { SetStdHandle(STD_ERROR_HANDLE, log.as_raw_handle()) } == 0 {
            return Err(std::io::Error::last_os_error())
                .context("Cannot redirect research daemon diagnostics");
        }
        Ok(log)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn quotes_explicit_data_homes_and_executables() {
            assert_eq!(quote(r"C:\app with spaces\"), "\"C:\\app with spaces\\\\\"");
            assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        }
    }
}
