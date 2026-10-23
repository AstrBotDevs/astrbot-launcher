use std::{env, error::Error, fs, path::PathBuf, process::Command};

fn main() -> Result<(), Box<dyn Error>> {
    if env::var("CARGO_CFG_TARGET_OS")? == "windows" {
        build_ctrlc_sender()?;
    }
    tauri_build::build();
    Ok(())
}

fn build_ctrlc_sender() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=ctrlc_sender/Cargo.toml");
    println!("cargo:rerun-if-changed=ctrlc_sender/src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("missing manifest dir")?);
    let target = env::var("TARGET")?;
    // A separate target directory avoids waiting on the outer Cargo build's lock.
    let target_dir =
        PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?).join("ctrlc-sender-target");
    let status = Command::new(env::var_os("CARGO").ok_or("missing CARGO")?)
        .current_dir(&manifest_dir)
        .args([
            "build",
            "--package",
            "ctrlc_sender",
            "--release",
            "--locked",
            "--target",
        ])
        .arg(&target)
        .arg("--target-dir")
        .arg(&target_dir)
        .status()?;
    if !status.success() {
        return Err(format!("failed to build ctrlc_sender for {target}: {status}").into());
    }

    let binary = fs::read(target_dir.join(&target).join("release/ctrlc_sender.exe"))?;
    let destination = manifest_dir
        .join("binaries")
        .join(format!("ctrlc_sender-{target}.exe"));
    fs::create_dir_all(manifest_dir.join("binaries"))?;
    // Tauri watches the sidecar too; avoid triggering rebuilds when it is unchanged.
    if fs::read(&destination).ok().as_deref() != Some(binary.as_slice()) {
        fs::write(destination, binary)?;
    }
    Ok(())
}
