//! Static HDL portability and warning-policy contracts.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn sv_files(directory: &Path) -> Vec<PathBuf> {
    fn visit(directory: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("cannot list {}: {error}", directory.display()))
        {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "sv") {
                found.push(path);
            }
        }
    }

    let mut found = Vec::new();
    visit(directory, &mut found);
    found.sort();
    found
}

#[test]
fn command_line_suppressions_match_reviewed_allowlist() {
    let makefile = read("scripts/Makefile");
    let suppression = Regex::new(r"-Wno-([A-Z0-9_]+)").unwrap();
    let found = suppression
        .captures_iter(&makefile)
        .map(|capture| capture[1].to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        found,
        BTreeSet::from(["UNUSEDPARAM".into(), "SYNCASYNCNET".into()])
    );
    assert!(makefile.contains("-Wall"));
    assert!(makefile.contains("--assert"));
    assert!(!makefile.contains("--Wno-fatal"));
}

#[test]
fn inline_suppressions_are_paired_and_reviewed() {
    let off = Regex::new(r"verilator lint_off ([A-Z0-9_]+)").unwrap();
    let on = Regex::new(r"verilator lint_on ([A-Z0-9_]+)").unwrap();
    let mut seen = BTreeSet::new();
    for root in ["rtl", "tb"] {
        for path in sv_files(&repo_root().join(root)) {
            let text = fs::read_to_string(&path).unwrap();
            let mut disabled = off
                .captures_iter(&text)
                .map(|capture| capture[1].to_owned())
                .collect::<Vec<_>>();
            let mut enabled = on
                .captures_iter(&text)
                .map(|capture| capture[1].to_owned())
                .collect::<Vec<_>>();
            disabled.sort();
            enabled.sort();
            assert_eq!(
                disabled,
                enabled,
                "unpaired lint pragma in {}",
                path.display()
            );
            seen.extend(disabled);
        }
    }
    assert_eq!(
        seen,
        BTreeSet::from([
            "DECLFILENAME".into(),
            "SYNCASYNCNET".into(),
            "UNUSEDSIGNAL".into(),
        ])
    );
}

#[test]
fn warning_policy_assigns_owner_rationale_and_review_trigger() {
    let policy = read("docs/WARNING_POLICY.md");
    for class in [
        "UNUSEDPARAM",
        "SYNCASYNCNET",
        "DECLFILENAME",
        "UNUSEDSIGNAL",
    ] {
        assert!(
            policy.contains(&format!("`{class}`")),
            "missing {class} policy"
        );
    }
    for field in ["Owner", "Rationale", "Review trigger"] {
        assert!(policy.contains(field), "missing {field} policy");
    }
}

#[test]
fn tests_never_use_nonfatal_systemverilog_error() {
    let offenders = ["rtl", "verification", "tb"]
        .into_iter()
        .flat_map(|root| sv_files(&repo_root().join(root)))
        .filter(|path| fs::read_to_string(path).unwrap().contains("$error"))
        .map(|path| {
            path.strip_prefix(repo_root())
                .unwrap()
                .display()
                .to_string()
        })
        .collect::<Vec<_>>();
    assert!(offenders.is_empty(), "unexpected $error in {offenders:?}");
}

#[test]
fn named_ports_are_not_connected_twice_in_one_instance() {
    let block_comments = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let line_comments = Regex::new(r"//[^\n]*").unwrap();
    let port = Regex::new(r"\.\s*([A-Za-z_]\w*)\s*\(").unwrap();
    let mut failures = Vec::new();
    for path in sv_files(&repo_root().join("rtl")) {
        let source = fs::read_to_string(&path).unwrap();
        let no_blocks = block_comments.replace_all(&source, "");
        let text = line_comments.replace_all(&no_blocks, "");
        let mut offset = 0;
        for statement in text.split(';') {
            let mut counts = BTreeMap::<String, usize>::new();
            for capture in port.captures_iter(statement) {
                *counts.entry(capture[1].to_owned()).or_default() += 1;
            }
            let duplicates = counts
                .into_iter()
                .filter_map(|(name, count)| (count > 1).then_some(name))
                .collect::<Vec<_>>();
            if !duplicates.is_empty() {
                let line = text[..offset].bytes().filter(|byte| *byte == b'\n').count() + 1;
                let relative = path.strip_prefix(repo_root()).unwrap();
                failures.push(format!(
                    "{}:{line}: duplicate named connection(s): {}",
                    relative.display(),
                    duplicates.join(", ")
                ));
            }
            offset += statement.len() + 1;
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
