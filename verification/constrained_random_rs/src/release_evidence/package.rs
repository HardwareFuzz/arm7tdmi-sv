//! Atomic release-manifest, archive, and checksum publication.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::{json, Value};
use tar::{Builder, HeaderMode};
use tempfile::NamedTempFile;

use super::{build_manifest, file_entry, ManifestRequest};

pub(super) fn git_output(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("release output has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    temporary
        .write_all(bytes)
        .map_err(|error| error.to_string())?;
    temporary.flush().map_err(|error| error.to_string())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

fn archive_files(
    root: &Path,
    candidates: &BTreeSet<PathBuf>,
    manifest_path: &Path,
    output: &Path,
    manifest: &Value,
) -> Result<String, String> {
    let expected = manifest["files"]
        .as_array()
        .ok_or("release manifest has no files")?
        .iter()
        .map(|entry| {
            let path = entry["path"]
                .as_str()
                .ok_or("release manifest file lacks path")?;
            Ok((path.to_owned(), entry.clone()))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    if expected.len() != candidates.len() {
        return Err("release manifest file list does not match archive candidates".into());
    }
    let parent = output.parent().ok_or("release archive has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    {
        let encoder = GzEncoder::new(temporary.as_file_mut(), Compression::default());
        let mut archive = Builder::new(encoder);
        archive.mode(HeaderMode::Deterministic);
        for path in candidates {
            let entry = file_entry(path, root)?;
            let relative = entry["path"]
                .as_str()
                .ok_or("release candidate has no path")?;
            if expected.get(relative) != Some(&entry) {
                return Err(format!(
                    "release candidate changed after hashing: {relative}"
                ));
            }
            archive
                .append_path_with_name(path, relative)
                .map_err(|error| error.to_string())?;
        }
        let manifest_relative = manifest_path
            .strip_prefix(root)
            .map_err(|_| "release manifest escapes repository")?;
        archive
            .append_path_with_name(manifest_path, manifest_relative)
            .map_err(|error| error.to_string())?;
        archive.finish().map_err(|error| error.to_string())?;
        archive
            .into_inner()
            .map_err(|error| error.to_string())?
            .finish()
            .map_err(|error| error.to_string())?;
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary
        .persist(output)
        .map_err(|error| error.to_string())?;
    let digest = super::sha256_bytes(fs::read(output).map_err(|error| error.to_string())?);
    let checksum_path = PathBuf::from(format!("{}.sha256", output.display()));
    let output_name = output
        .file_name()
        .ok_or("release archive has no filename")?;
    atomic_write(
        &checksum_path,
        format!("{digest}  {}\n", output_name.to_string_lossy()).as_bytes(),
    )?;
    Ok(digest)
}

fn report_paths(root: &Path, request: &ManifestRequest) -> (PathBuf, PathBuf) {
    let resolve = |path: Option<&PathBuf>, default: &str| match path {
        Some(path) if path.is_absolute() => path.clone(),
        Some(path) => root.join(path),
        None => root.join(default),
    };
    (
        resolve(
            request.regression_path.as_ref(),
            "reports/generated/regression.json",
        ),
        resolve(
            request.coverage_path.as_ref(),
            "reports/generated/coverage/coverage.json",
        ),
    )
}

pub(super) fn package(root: &Path, request: &ManifestRequest) -> Result<Value, String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let actual_commit = git_output(&root, &["rev-parse", "HEAD"])?.trim().to_owned();
    if request.git_commit != actual_commit || request.git_commit.len() < 12 {
        return Err("release package commit does not match HEAD".into());
    }
    if !git_output(
        &root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?
    .trim()
    .is_empty()
    {
        return Err("current source tree is dirty".into());
    }
    let source_digest =
        crate::coverage_report::source_digest(&root).map_err(|error| error.to_string())?;
    if request.source_sha256 != source_digest {
        return Err("current source digest does not match evidence".into());
    }
    let mut candidates = BTreeSet::new();
    for path in &request.candidates {
        let resolved = path.canonicalize().map_err(|error| error.to_string())?;
        if !resolved.starts_with(&root) || !resolved.is_file() {
            return Err(format!("invalid release candidate: {}", path.display()));
        }
        candidates.insert(resolved);
    }
    let (regression_path, coverage_path) = report_paths(&root, request);
    for required in [&regression_path, &coverage_path] {
        let required = required.canonicalize().map_err(|error| error.to_string())?;
        if !candidates.contains(&required) {
            return Err(format!("release package is missing {}", required.display()));
        }
    }
    let regression: Value =
        serde_json::from_slice(&fs::read(&regression_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let coverage: Value =
        serde_json::from_slice(&fs::read(&coverage_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    super::validate_basic(&regression, &coverage, &request.git_commit)?;
    if regression["git"]["source_sha256"] != request.source_sha256
        || coverage["git"]["source_sha256"] != request.source_sha256
    {
        return Err("release reports do not match the source digest".into());
    }

    let manifest = build_manifest(&root, request)?;
    let manifest_path = root.join("reports/generated/release-manifest.json");
    let mut manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    manifest_bytes.push(b'\n');
    atomic_write(&manifest_path, &manifest_bytes)?;
    let default_name = format!(
        "arm7tdmis-release-evidence-{}.tar.gz",
        &request.git_commit[..12]
    );
    let output = request
        .output
        .clone()
        .unwrap_or_else(|| root.join("reports/generated").join(default_name));
    let output = if output.is_absolute() {
        output
    } else {
        root.join(output)
    };
    if candidates.contains(&output) || output == manifest_path {
        return Err("release archive path conflicts with a hashed evidence file".into());
    }
    let digest = archive_files(&root, &candidates, &manifest_path, &output, &manifest)?;
    Ok(json!({
        "output": output,
        "manifest": manifest_path,
        "sha256": digest,
        "file_count": candidates.len() + 1,
    }))
}

pub(super) fn command() -> Result<String, String> {
    let root = crate::campaign::repo_root();
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())?;
    let request: ManifestRequest = serde_json::from_str(&input)
        .map_err(|error| format!("invalid release package request: {error}"))?;
    let (regression_path, coverage_path) = report_paths(&root, &request);
    let regression: Value =
        serde_json::from_slice(&fs::read(&regression_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let coverage: Value =
        serde_json::from_slice(&fs::read(&coverage_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let expected = super::files::validate_prefix(
        &root,
        &regression,
        &coverage,
        &regression_path,
        &coverage_path,
    )?;
    let supplied = request
        .candidates
        .iter()
        .map(|path| path.canonicalize().map_err(|error| error.to_string()))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if supplied != expected {
        return Err("release package candidate list does not match validated evidence".into());
    }
    serde_json::to_string(&package(&root, &request)?).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn git(root: &Path, arguments: &[&str]) {
        let status = Command::new("git")
            .args(arguments)
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success(), "git {arguments:?} failed");
    }

    #[test]
    fn archive_contains_exact_hashed_candidates_and_manifest() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let input = root.join("input.bin");
        fs::write(&input, b"evidence").unwrap();
        let manifest_path = root.join("release-manifest.json");
        fs::write(&manifest_path, b"{\"schema\":\"test\"}\n").unwrap();
        let expected = file_entry(&input, root).unwrap();
        let manifest = json!({"files": [expected]});
        let output = root.join("archive.tar.gz");
        archive_files(
            root,
            &BTreeSet::from([input.clone()]),
            &manifest_path,
            &output,
            &manifest,
        )
        .unwrap();
        let decoder = flate2::read::GzDecoder::new(File::open(&output).unwrap());
        let mut archive = tar::Archive::new(decoder);
        let names = archive
            .entries()
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(names, ["input.bin", "release-manifest.json"]);
        assert!(root.join("archive.tar.gz.sha256").is_file());
        fs::write(&input, b"changed").unwrap();
        assert!(archive_files(
            root,
            &BTreeSet::from([input]),
            &manifest_path,
            &output,
            &manifest
        )
        .is_err());
    }

    #[test]
    fn clean_release_package_is_complete_and_rejects_stale_reports() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        fs::create_dir_all(root.join("verification")).unwrap();
        fs::write(root.join("verification/sample.rs"), b"pub fn sample() {}\n").unwrap();
        fs::write(root.join("VERSION"), b"0.1.0\n").unwrap();
        fs::write(root.join("LICENSE"), b"license\n").unwrap();
        fs::write(
            root.join("ARM_DDI_0234B_ARM7TDMI-S_r4p3_TRM.pdf"),
            b"specification\n",
        )
        .unwrap();
        fs::write(root.join(".gitignore"), b"/reports/generated/\n").unwrap();
        git(root, &["init", "-q"]);
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "test",
            ],
        );
        let commit = git_output(root, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_owned();
        let source_sha256 = crate::coverage_report::source_digest(root).unwrap();
        let report_root = root.join("reports/generated");
        let regression_path = report_root.join("regression.json");
        let coverage_path = report_root.join("coverage/coverage.json");
        fs::create_dir_all(coverage_path.parent().unwrap()).unwrap();
        let regression = json!({
            "schema": "arm7tdmis-regression-v1",
            "status": "passed",
            "git": {"commit": commit, "dirty": false, "source_sha256": source_sha256},
        });
        let coverage = json!({
            "schema": "arm7tdmis-coverage-v1",
            "git": {"commit": commit, "dirty": false, "source_sha256": source_sha256},
        });
        fs::write(&regression_path, serde_json::to_vec(&regression).unwrap()).unwrap();
        fs::write(&coverage_path, serde_json::to_vec(&coverage).unwrap()).unwrap();
        let request = ManifestRequest {
            candidates: vec![regression_path.clone(), coverage_path],
            git_commit: commit.clone(),
            source_sha256,
            tools: BTreeMap::new(),
            created_utc: "2026-01-01T00:00:00+00:00".into(),
            python_executable: None,
            output: None,
            regression_path: None,
            coverage_path: None,
        };
        let result = package(root, &request).unwrap();
        let output = result["output"].as_str().unwrap();
        assert!(Path::new(output).is_file());
        assert_eq!(result["file_count"], 3);
        assert!(report_root.join("release-manifest.json").is_file());
        assert!(PathBuf::from(format!("{output}.sha256")).is_file());

        let decoder = flate2::read::GzDecoder::new(File::open(output).unwrap());
        let mut archive = tar::Archive::new(decoder);
        let names = archive
            .entries()
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from([
                "reports/generated/regression.json".to_owned(),
                "reports/generated/coverage/coverage.json".to_owned(),
                "reports/generated/release-manifest.json".to_owned(),
            ])
        );
        let mut stale = regression;
        stale["git"]["commit"] = json!("wrong");
        fs::write(&regression_path, serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!(package(root, &request).is_err());
    }
}
