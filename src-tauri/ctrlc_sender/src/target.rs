use std::ffi::OsString;
use std::fs;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use windows::core::{Owned, PWSTR};
use windows::Win32::Foundation::{HANDLE, STILL_ACTIVE};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

// Keep these locations in sync with the launcher's utils/paths.rs. In
// particular, never accept a data directory or executable allowlist from argv.
fn data_dir() -> Result<PathBuf, String> {
    let config = dirs::config_dir()
        .ok_or("Cannot locate launcher configuration")?
        .join("com.github.raven95676.astrbot-launcher")
        .join(".data-dir.json");
    match fs::read_to_string(config) {
        Ok(content) => {
            let value: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| e.to_string())?;
            let path = value["data_dir"]
                .as_str()
                .ok_or("Invalid data directory configuration")?;
            let path = PathBuf::from(path.trim());
            if !path.is_absolute() || !path.is_dir() {
                return Err("Invalid data directory configuration".into());
            }
            Ok(path)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(dirs::home_dir()
            .ok_or("Cannot locate home directory")?
            .join(".astrbot_launcher")),
        Err(e) => Err(format!("Cannot read launcher configuration: {e}")),
    }
}

fn normalized(path: &Path) -> Vec<u16> {
    // Work on UTF-16 directly so distinct non-Unicode paths never compare equal
    // through lossy string conversion. Both inputs must already be canonical.
    path.as_os_str()
        .encode_wide()
        .map(|c| match c {
            65..=90 => c + 32,
            _ => c,
        })
        .collect()
}

fn is_allowed_executable(root: &Path, executable: &Path) -> bool {
    let root = normalized(root);
    let executable = normalized(executable);
    let Some(relative) = executable.strip_prefix(root.as_slice()) else {
        return false;
    };
    // Require a directory boundary (also handles a configured volume root).
    let relative = if root.last() == Some(&92) {
        relative
    } else if let Some(relative) = relative.strip_prefix(&[92]) {
        relative
    } else {
        return false;
    };
    let parts: Vec<&[u16]> = relative.split(|c| *c == 92).collect();
    let eq = |part: &[u16], expected: &str| part.iter().copied().eq(expected.encode_utf16());
    match parts.as_slice() {
        [components, python, runtime, exe] => {
            eq(components, "components")
                && eq(python, "python")
                && (eq(runtime, "py310") || eq(runtime, "py312"))
                && eq(exe, "python.exe")
        }
        [instances, id, venv, scripts, exe] => {
            eq(instances, "instances")
                && !id.is_empty()
                && !eq(id, ".")
                && !eq(id, "..")
                && eq(venv, "venv")
                && eq(scripts, "scripts")
                && eq(exe, "python.exe")
        }
        _ => false,
    }
}

fn validate_executable(root: &Path, executable: &Path) -> Result<(), String> {
    // Resolve junctions/symlinks and reject any executable escaping the
    // configured data root. Failure to resolve is always a denial.
    let executable = fs::canonicalize(executable).map_err(|e| e.to_string())?;
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    if !is_allowed_executable(&root, &executable) {
        return Err(format!(
            "Executable is outside managed Python paths: {}",
            executable.display()
        ));
    }
    Ok(())
}

pub(super) struct ValidatedTarget(Owned<HANDLE>);

impl ValidatedTarget {
    pub(super) fn open(pid: u32) -> Result<Self, String> {
        // 0 and MAX have special meanings to Windows APIs; neither is a target.
        if pid == 0 || pid == u32::MAX {
            return Err("Invalid target PID".into());
        }
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map_err(|e| format!("Cannot open target: {e}"))?;
        let target = Self(unsafe { Owned::new(handle) });
        let mut buffer = vec![0u16; 32768];
        let mut len = buffer.len() as u32;
        unsafe {
            QueryFullProcessImageNameW(
                *target.0,
                PROCESS_NAME_FORMAT(0),
                PWSTR(buffer.as_mut_ptr()),
                &mut len,
            )
        }
        .map_err(|e| format!("Cannot query target executable: {e}"))?;
        let executable = PathBuf::from(OsString::from_wide(&buffer[..len as usize]));
        validate_executable(&data_dir()?, &executable)?;
        target.ensure_alive()?;
        Ok(target)
    }

    pub(super) fn ensure_alive(&self) -> Result<(), String> {
        let mut code = 0;
        unsafe { GetExitCodeProcess(*self.0, &mut code) }.map_err(|e| e.to_string())?;
        if code != STILL_ACTIVE.0 as u32 {
            return Err("Target process has exited".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_only_managed_python_layouts() {
        let root = Path::new(r"\\?\C:\Launcher Data");
        for relative in [
            r"components\python\py310\python.exe",
            r"components\python\py312\python.exe",
            r"instances\instance-id\venv\Scripts\python.exe",
        ] {
            assert!(
                is_allowed_executable(root, &root.join(relative)),
                "{relative}"
            );
        }
        for relative in [
            r"python.exe",
            r"components\python\py313\python.exe",
            r"components\python\py310\pythonw.exe",
            r"components\nodejs\node.exe",
            r"instances\id\core\python.exe",
            r"instances\id\venv\Scripts\cmd.exe",
            r"instances\id\venv\Scripts\extra\python.exe",
            r"instances\..\venv\Scripts\python.exe",
        ] {
            assert!(
                !is_allowed_executable(root, &root.join(relative)),
                "{relative}"
            );
        }
    }

    #[test]
    fn rejects_other_roots_and_prefix_collisions() {
        let root = Path::new(r"\\?\C:\data");
        for path in [
            r"\\?\C:\data-other\components\python\py310\python.exe",
            r"\\?\D:\data\components\python\py310\python.exe",
            r"\\?\C:\external\python.exe",
        ] {
            assert!(!is_allowed_executable(root, Path::new(path)));
        }
        assert!(is_allowed_executable(
            root,
            Path::new(r"\\?\c:\DATA\Components\Python\PY312\PYTHON.EXE")
        ));
    }

    #[test]
    fn rejects_special_and_unmanaged_processes() {
        assert!(ValidatedTarget::open(0).is_err());
        assert!(ValidatedTarget::open(u32::MAX).is_err());
        assert!(ValidatedTarget::open(std::process::id()).is_err());
    }

    #[test]
    fn canonicalizes_before_checking_and_denies_unresolvable_paths() {
        let temp = std::env::temp_dir().join(format!("ctrlc-sender-test-{}", std::process::id()));
        let root = temp.join("data");
        let python_dir = root.join(r"components\python\py310");
        fs::create_dir_all(&python_dir).unwrap();
        let python = python_dir.join("python.exe");
        fs::write(&python, b"test fixture").unwrap();
        assert!(validate_executable(&root, &python).is_ok());
        assert!(validate_executable(&root, &python_dir.join("missing.exe")).is_err());
        // A lexical allowlist alone would see the permitted prefix, but the
        // canonical path must reject the executable outside that directory.
        let outside = temp.join("python.exe");
        fs::write(&outside, b"test fixture").unwrap();
        assert!(validate_executable(&root, &python_dir.join(r"..\..\..\..\python.exe")).is_err());
        fs::remove_file(python).unwrap();
        fs::remove_file(outside).unwrap();
        // Only remove the known, empty fixture directories.
        for dir in [
            python_dir,
            root.join(r"components\python"),
            root.join("components"),
            root,
            temp,
        ] {
            fs::remove_dir(dir).unwrap();
        }
    }
}
