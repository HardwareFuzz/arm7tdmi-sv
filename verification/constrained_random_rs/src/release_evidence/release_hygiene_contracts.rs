//! Versioned public API and release-document contracts, including a real
//! manifest build. The Rust harness invokes the builder directly.

use std::collections::BTreeMap;
use std::fs;

use regex::Regex;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{build_manifest, ManifestRequest, MANIFEST_SCHEMA, TRM};
use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

#[test]
fn semantic_version_and_release_documents_are_present() {
    let version = read("VERSION");
    let version = version.trim();
    assert!(
        Regex::new(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?$")
            .unwrap()
            .is_match(version)
    );
    for (path, markers) in [
        ("CHANGELOG.md", vec!["Unreleased", version]),
        (
            "docs/SUPPORT.md",
            vec!["Support matrix", "Verilator", "Quartus"],
        ),
        ("docs/LIMITATIONS.md", vec!["Known limitations", "TASKS.md"]),
        ("SECURITY.md", vec!["Security", "debug"]),
        (
            "docs/PROVENANCE.md",
            vec!["SPDX", "GPL-3.0-only", "NOASSERTION"],
        ),
    ] {
        let contents = read(path);
        for marker in markers {
            assert!(contents.contains(marker), "{path} is missing {marker}");
        }
    }
}

#[test]
fn both_public_apis_keep_complete_versioned_sections() {
    for (path, headings) in [
        (
            "docs/INTEGRATION.md",
            vec![
                "Clock and reset",
                "Memory request",
                "Event CDC",
                "Optional interfaces",
                "Public memory-bus adapters",
                "Portable FPGA package",
                "Error behavior",
                "Version and compatibility",
            ],
        ),
        (
            "docs/RAW_BUS.md",
            vec![
                "Edge and phase model",
                "Clock enable and wait states",
                "Address, size, and lanes",
                "ABORT",
                "LOCK and DMORE",
                "Coprocessor mirrors",
                "Integration example",
                "Version and compatibility",
            ],
        ),
    ] {
        let contents = read(path);
        for heading in headings {
            let pattern = format!(r"(?m)^## .*{}", regex::escape(heading));
            assert!(
                Regex::new(&pattern).unwrap().is_match(&contents),
                "{path} is missing section {heading}"
            );
        }
    }
}

fn request(tools: BTreeMap<String, String>) -> ManifestRequest {
    ManifestRequest {
        candidates: vec![repo_root().join("LICENSE")],
        git_commit: "a".repeat(40),
        source_sha256: "b".repeat(64),
        tools,
        created_utc: "2026-01-01T00:00:00+00:00".into(),
        python_executable: None,
        output: None,
        regression_path: None,
        coverage_path: None,
    }
}

#[test]
fn release_manifest_hashes_source_tools_spec_and_files() {
    let root = repo_root();
    let tools = BTreeMap::from([
        ("verilator".to_owned(), "Verilator unit".to_owned()),
        ("quartus".to_owned(), "Quartus unit".to_owned()),
        ("python".to_owned(), "Python unit".to_owned()),
    ]);
    let manifest = build_manifest(&root, &request(tools.clone())).unwrap();
    assert_eq!(manifest["schema"], MANIFEST_SCHEMA);
    assert_eq!(manifest["version"], read("VERSION").trim());
    assert_eq!(manifest["source"]["git_commit"], "a".repeat(40));
    assert_eq!(manifest["source"]["source_sha256"], "b".repeat(64));
    let sha256 = Regex::new(r"^[0-9a-f]{64}$").unwrap();
    assert!(sha256.is_match(manifest["source"]["git_tree_sha256"].as_str().unwrap()));
    assert_eq!(manifest["tools"]["versions"], json!(tools));
    let expected_tool_digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&tools).unwrap()));
    assert_eq!(manifest["tools"]["versions_sha256"], expected_tool_digest);
    let specifications = manifest["specifications"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["path"].as_str().unwrap(),
                entry["sha256"].as_str().unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        specifications[TRM],
        "bb9cd0e3f2b7e2fdca4ff961cdfc5c9c85c063842e491f8997a504d3241baa14"
    );
    assert!(sha256.is_match(specifications["LICENSE"]));
    assert_eq!(manifest["files"][0]["path"], "LICENSE");

    let reversed = BTreeMap::from_iter(tools.into_iter().rev());
    let reversed_manifest = build_manifest(&root, &request(reversed)).unwrap();
    assert_eq!(
        manifest["tools"]["versions_sha256"],
        reversed_manifest["tools"]["versions_sha256"]
    );
}

#[test]
fn task_ledger_links_release_hygiene_evidence() {
    let tasks = read("TASKS.md");
    for id in ["DOC-004", "DOC-005", "DOC-006"] {
        assert!(tasks.contains(&format!("- [x] **{id}:**")));
    }
    for marker in [
        "PROVENANCE.md",
        "SUPPORT.md",
        "LIMITATIONS.md",
        "SECURITY.md",
        "release-manifest.json",
        "verification/constrained_random_rs/src/release_evidence/release_hygiene_contracts.rs",
    ] {
        assert!(tasks.contains(marker), "task ledger is missing {marker}");
    }
}
