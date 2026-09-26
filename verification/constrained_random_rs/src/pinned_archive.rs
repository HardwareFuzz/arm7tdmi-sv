//! Shared, checksum-gated download and extraction for pinned local tool suites.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

pub fn run(command: &mut Command) -> Result<String, String> {
    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "command {:?} failed ({}): {}{}",
            command,
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

pub fn file_sha256(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn member_is_safe(name: &str, directory: &str) -> bool {
    let mut components = Path::new(name).components();
    let first = components.find(|part| *part != Component::CurDir);
    if first != Some(Component::Normal(directory.as_ref())) {
        return false;
    }
    components.all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
}

pub enum Compression {
    Gzip,
    Xz,
}

pub fn extract_checked(
    archive: &Path,
    destination: &Path,
    directory: &str,
    compression: Compression,
) -> Result<PathBuf, String> {
    // The caller must verify the pinned SHA-256 before invoking this function.
    let (list_flag, extract_flag) = match compression {
        Compression::Gzip => ("-tzf", "-xzf"),
        Compression::Xz => ("-tJf", "-xJf"),
    };
    let listing = run(Command::new("tar").arg(list_flag).arg(archive))?;
    if listing.lines().next().is_none() {
        return Err("toolchain archive is empty".into());
    }
    for member in listing.lines() {
        if !member_is_safe(member, directory) {
            return Err(format!("unsafe archive member: {member}"));
        }
    }
    run(Command::new("tar")
        .arg(extract_flag)
        .arg(archive)
        .arg("-C")
        .arg(destination)
        .arg("--no-same-owner"))?;
    Ok(destination.join(directory))
}

pub fn download(url: &str, output: &Path, max_seconds: u32) -> Result<(), String> {
    run(Command::new("curl")
        .arg("--fail")
        .arg("--location")
        .arg("--silent")
        .arg("--show-error")
        .arg("--connect-timeout")
        .arg("30")
        .arg("--max-time")
        .arg(max_seconds.to_string())
        .arg("--output")
        .arg(output)
        .arg(url))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_paths_cannot_escape_expected_directory() {
        assert!(member_is_safe("./tool/bin/gcc", "tool"));
        assert!(member_is_safe("tool", "tool"));
        assert!(!member_is_safe("../escape", "tool"));
        assert!(!member_is_safe("/absolute/path", "tool"));
        assert!(!member_is_safe("tool/../escape", "tool"));
        assert!(!member_is_safe("other/bin/gcc", "tool"));
    }
}
