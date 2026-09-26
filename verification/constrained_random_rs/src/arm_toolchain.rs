//! Install the checksum-pinned Arm GNU toolchain used by compiler evidence.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use crate::campaign::repo_root;
use crate::pinned_archive::{download, extract_checked, file_sha256, run, Compression};

const RELEASE: &str = "14.3.rel1";
const ARCHIVE: &str = "arm-gnu-toolchain-14.3.rel1-x86_64-arm-none-eabi.tar.xz";
const DIRECTORY: &str = "arm-gnu-toolchain-14.3.rel1-x86_64-arm-none-eabi";
const URL: &str = "https://developer.arm.com/-/media/Files/downloads/gnu/14.3.rel1/binrel/arm-gnu-toolchain-14.3.rel1-x86_64-arm-none-eabi.tar.xz";
const SHA256: &str = "8f6903f8ceb084d9227b9ef991490413014d991874a1e34074443c2a72b14dbd";
const SCHEMA: &str = "arm7tdmis-arm-toolchain-v1";

fn compiler_version(install: &Path) -> Result<String, String> {
    let output = run(Command::new(install.join("bin/arm-none-eabi-gcc")).arg("--version"))?;
    Ok(output.lines().next().unwrap_or_default().to_owned())
}

fn metadata() -> Value {
    json!({
        "archive": ARCHIVE,
        "archive_sha256": SHA256,
        "release": RELEASE,
        "schema": SCHEMA,
        "url": URL,
    })
}

fn existing_install_is_valid(install: &Path) -> bool {
    let path = install.join("metadata.json");
    if !path.is_file() || !install.join("bin/arm-none-eabi-gcc").is_file() {
        return false;
    }
    let Ok(contents) = fs::read(&path) else {
        return false;
    };
    let Ok(actual) = serde_json::from_slice::<Value>(&contents) else {
        return false;
    };
    let Ok(version) = compiler_version(install) else {
        return false;
    };
    actual["schema"] == SCHEMA
        && actual["release"] == RELEASE
        && actual["archive_sha256"] == SHA256
        && version.contains("14.3.1")
}

fn install_toolchain(root: &Path) -> Result<String, String> {
    if std::env::consts::OS != "linux" || std::env::consts::ARCH != "x86_64" {
        return Err("the pinned binary toolchain supports Linux x86_64 only".into());
    }
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    // Multiple Make targets may request the same toolchain concurrently.
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".arm-toolchain.lock"))
        .map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;

    let install = root.join(DIRECTORY);
    if existing_install_is_valid(&install) {
        return Ok(format!(
            "[arm-toolchain] PASS cached {}",
            compiler_version(&install)?
        ));
    }

    let staging = tempfile::Builder::new()
        .prefix(".arm-toolchain-")
        .tempdir_in(&root)
        .map_err(|error| error.to_string())?;
    let archive = staging.path().join(ARCHIVE);
    println!("[arm-toolchain] downloading {URL}");
    download(URL, &archive, 300)?;
    let observed = file_sha256(&archive)?;
    if observed != SHA256 {
        return Err(format!(
            "toolchain SHA-256 expected {SHA256}, got {observed}"
        ));
    }

    let extract_root = staging.path().join("extract");
    fs::create_dir(&extract_root).map_err(|error| error.to_string())?;
    let extracted = extract_checked(&archive, &extract_root, DIRECTORY, Compression::Xz)?;
    if !extracted.join("bin/arm-none-eabi-gcc").is_file() {
        return Err("toolchain archive has an unexpected layout".into());
    }
    let version = compiler_version(&extracted)?;
    if !version.contains("14.3.1") {
        return Err(format!("unexpected Arm GNU compiler version: {version}"));
    }
    let metadata_path = extracted.join("metadata.json");
    let mut serialized = serde_json::to_string_pretty(&metadata()).map_err(|e| e.to_string())?;
    serialized.push('\n');
    fs::write(metadata_path, serialized).map_err(|error| error.to_string())?;

    // Keep an invalid previous install available until the replacement is in place.
    let backup = staging.path().join("previous-install");
    if install.exists() {
        fs::rename(&install, &backup).map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(&extracted, &install) {
        if backup.exists() {
            fs::rename(&backup, &install).map_err(|restore| {
                format!("install failed: {error}; restoring previous install failed: {restore}")
            })?;
        }
        return Err(error.to_string());
    }
    Ok(format!("[arm-toolchain] PASS installed {version}"))
}

pub fn command(arguments: &[String]) -> Result<String, String> {
    let mut root = repo_root().join(".tools");
    let mut args = arguments.iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => root = PathBuf::from(args.next().ok_or("missing --root value")?),
            other => return Err(format!("unknown arm-toolchain-install argument: {other}")),
        }
    }
    install_toolchain(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_metadata_contract() {
        let value = metadata();
        assert_eq!(value["release"], RELEASE);
        assert_eq!(value["archive_sha256"], SHA256);
        assert_eq!(value["schema"], SCHEMA);
    }

    #[test]
    #[ignore = "downloads and extracts the full pinned Arm GNU release"]
    fn fresh_install_then_cached_install() {
        let root = tempfile::tempdir().unwrap();
        let installed = install_toolchain(root.path()).unwrap();
        assert!(installed.contains("PASS installed"));
        let cached = install_toolchain(root.path()).unwrap();
        assert!(cached.contains("PASS cached"));
    }
}
