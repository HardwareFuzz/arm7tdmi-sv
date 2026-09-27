//! End-to-end release evidence gate. The report validators and packager remain
//! separate so neither a clean tree nor a manifest alone can imply sign-off.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{files, package, validate_basic, ManifestRequest};

struct Options {
    regression: PathBuf,
    coverage: PathBuf,
    output: Option<PathBuf>,
}

fn absolute(path: PathBuf) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path))
    }
}

fn options(arguments: &[String], root: &Path) -> Result<Options, String> {
    let mut result = Options {
        regression: root.join("reports/generated/regression.json"),
        coverage: root.join("reports/generated/coverage/coverage.json"),
        output: None,
    };
    let mut position = 0;
    while position < arguments.len() {
        let value = arguments
            .get(position + 1)
            .ok_or_else(|| format!("missing value for {}", arguments[position]))?;
        match arguments[position].as_str() {
            "--regression" => result.regression = absolute(PathBuf::from(value))?,
            "--coverage" => result.coverage = absolute(PathBuf::from(value))?,
            "--output" => result.output = Some(absolute(PathBuf::from(value))?),
            unknown => return Err(format!("unknown release-evidence option: {unknown}")),
        }
        position += 2;
    }
    Ok(result)
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn command(arguments: &[String]) -> Result<String, String> {
    let root = crate::campaign::repo_root()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let options = options(arguments, &root)?;
    let commit = package::git_output(&root, &["rev-parse", "HEAD"])?
        .trim()
        .to_owned();
    if !package::git_output(
        &root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?
    .trim()
    .is_empty()
    {
        return Err("current source tree is dirty".into());
    }
    let regression = read_json(&options.regression)?;
    let coverage = read_json(&options.coverage)?;
    validate_basic(&regression, &coverage, &commit)?;
    let source = regression["git"]["source_sha256"]
        .as_str()
        .ok_or("regression report has no source digest")?;
    if coverage["git"]["source_sha256"] != source {
        return Err("regression and coverage source digests do not match".into());
    }
    let current_digest =
        crate::coverage_report::source_digest(&root).map_err(|error| error.to_string())?;
    if source != current_digest {
        return Err("current source digest does not match evidence".into());
    }
    let candidates = files::validate_prefix(
        &root,
        &regression,
        &coverage,
        &options.regression,
        &options.coverage,
    )?;
    let tools: BTreeMap<String, String> = serde_json::from_value(regression["tools"].clone())
        .map_err(|error| format!("regression tool manifest is malformed: {error}"))?;
    let request = ManifestRequest {
        candidates: candidates.into_iter().collect(),
        git_commit: commit,
        source_sha256: source.to_owned(),
        tools,
        created_utc: crate::campaign::utc_now(),
        output: options.output,
        regression_path: Some(options.regression),
        coverage_path: Some(options.coverage),
    };
    let result = package::package(&root, &request)?;
    Ok(format!(
        "[release-evidence] wrote {} with {} hashed files",
        result["output"]
            .as_str()
            .ok_or("release archive path is missing")?,
        result["file_count"]
            .as_u64()
            .ok_or("release file count is missing")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_option_without_starting_release_gate() {
        let root = Path::new("/tmp/arm7-release-gate");
        assert!(options(&["--unknown".into(), "value".into()], root).is_err());
        assert!(options(&["--coverage".into()], root).is_err());
        let defaults = options(&[], root).unwrap();
        assert_eq!(
            defaults.regression,
            root.join("reports/generated/regression.json")
        );
    }
}
