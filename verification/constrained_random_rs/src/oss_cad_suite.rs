//! Install the pinned open-source synthesis and formal tool suite.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use crate::campaign::repo_root;
use crate::pinned_archive::{download, extract_checked, file_sha256, run, Compression};

const RELEASE: &str = "2026-07-29";
const ARCHIVE: &str = "oss-cad-suite-linux-x64-20260729.tgz";
const URL: &str = "https://github.com/YosysHQ/oss-cad-suite-build/releases/download/2026-07-29/oss-cad-suite-linux-x64-20260729.tgz";
const SHA256: &str = "89ea1152ea84bc600f18cc685f721d534d1f018e09831662787865a3d79ce4aa";
const EXTRACTED_DIRECTORY: &str = "oss-cad-suite";
const INSTALL_DIRECTORY: &str = "oss-cad-suite-20260729";
const SCHEMA: &str = "arm7tdmis-oss-cad-suite-v1";

fn metadata() -> Value {
    json!({
        "archive": ARCHIVE,
        "archive_sha256": SHA256,
        "release": RELEASE,
        "schema": SCHEMA,
        "url": URL,
    })
}

fn versions(install: &Path) -> Result<BTreeMap<&'static str, String>, String> {
    let mut versions = BTreeMap::new();
    for name in ["sby", "yosys", "boolector"] {
        let output = run(Command::new(install.join("bin").join(name)).arg("--version"))?;
        versions.insert(name, output.lines().next().unwrap_or_default().to_owned());
    }
    Ok(versions)
}

fn versions_match(versions: &BTreeMap<&str, String>) -> bool {
    versions
        .get("sby")
        .is_some_and(|version| version.contains("SBY v0.67"))
        && versions
            .get("yosys")
            .is_some_and(|version| version.contains("Yosys 0.67"))
        && versions
            .get("boolector")
            .is_some_and(|version| version == "3.2.4")
}

fn display_versions(versions: &BTreeMap<&str, String>) -> String {
    format!(
        "{}; {}; Boolector {}",
        versions.get("sby").map(String::as_str).unwrap_or_default(),
        versions
            .get("yosys")
            .map(String::as_str)
            .unwrap_or_default(),
        versions
            .get("boolector")
            .map(String::as_str)
            .unwrap_or_default()
    )
}

fn existing_install_is_valid(install: &Path) -> bool {
    let path = install.join("metadata.json");
    let Ok(contents) = fs::read(path) else {
        return false;
    };
    let Ok(actual) = serde_json::from_slice::<Value>(&contents) else {
        return false;
    };
    let Ok(versions) = versions(install) else {
        return false;
    };
    actual["schema"] == SCHEMA
        && actual["release"] == RELEASE
        && actual["archive_sha256"] == SHA256
        && versions_match(&versions)
}

fn install_suite(root: &Path) -> Result<String, String> {
    if std::env::consts::OS != "linux" || std::env::consts::ARCH != "x86_64" {
        return Err("the pinned OSS CAD Suite binary supports Linux x86_64 only".into());
    }
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".oss-cad-suite.lock"))
        .map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;

    let install = root.join(INSTALL_DIRECTORY);
    if existing_install_is_valid(&install) {
        return Ok(format!(
            "[oss-cad-suite] PASS cached {}",
            display_versions(&versions(&install)?)
        ));
    }

    let staging = tempfile::Builder::new()
        .prefix(".oss-cad-")
        .tempdir_in(&root)
        .map_err(|error| error.to_string())?;
    let cached_archive = root.join(ARCHIVE);
    let archive = if cached_archive.is_file() {
        println!(
            "[oss-cad-suite] using cached archive {}",
            cached_archive.display()
        );
        cached_archive
    } else {
        let downloaded = staging.path().join(ARCHIVE);
        println!("[oss-cad-suite] downloading {URL}");
        download(URL, &downloaded, 600)?;
        downloaded
    };
    let observed = file_sha256(&archive)?;
    if observed != SHA256 {
        return Err(format!(
            "toolchain SHA-256 expected {SHA256}, got {observed}"
        ));
    }

    let extract_root = staging.path().join("extract");
    fs::create_dir(&extract_root).map_err(|error| error.to_string())?;
    let extracted = extract_checked(
        &archive,
        &extract_root,
        EXTRACTED_DIRECTORY,
        Compression::Gzip,
    )?;
    if !extracted.join("bin/sby").is_file() {
        return Err("OSS CAD Suite archive has an unexpected layout".into());
    }
    let tool_versions = versions(&extracted)?;
    if !versions_match(&tool_versions) {
        return Err(format!(
            "OSS CAD Suite tool versions do not match the pinned release: {}",
            display_versions(&tool_versions)
        ));
    }
    let mut serialized =
        serde_json::to_string_pretty(&metadata()).map_err(|error| error.to_string())?;
    serialized.push('\n');
    fs::write(extracted.join("metadata.json"), serialized).map_err(|error| error.to_string())?;

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
    Ok(format!(
        "[oss-cad-suite] PASS installed {}",
        display_versions(&tool_versions)
    ))
}

pub fn command(arguments: &[String]) -> Result<String, String> {
    let mut root = repo_root().join(".tools");
    let mut args = arguments.iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => root = PathBuf::from(args.next().ok_or("missing --root value")?),
            other => return Err(format!("unknown oss-cad-suite-install argument: {other}")),
        }
    }
    install_suite(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_metadata_and_versions() {
        assert_eq!(metadata()["archive_sha256"], SHA256);
        assert_eq!(metadata()["release"], RELEASE);
        let versions = BTreeMap::from([
            ("sby", "SBY v0.67".to_owned()),
            ("yosys", "Yosys 0.67".to_owned()),
            ("boolector", "3.2.4".to_owned()),
        ]);
        assert!(versions_match(&versions));
    }

    #[test]
    #[ignore = "downloads and extracts the full pinned OSS CAD Suite release"]
    fn fresh_install_then_cached_install() {
        let root = tempfile::tempdir().unwrap();
        let installed = install_suite(root.path()).unwrap();
        assert!(installed.contains("PASS installed"));
        let cached = install_suite(root.path()).unwrap();
        assert!(cached.contains("PASS cached"));
    }
}
