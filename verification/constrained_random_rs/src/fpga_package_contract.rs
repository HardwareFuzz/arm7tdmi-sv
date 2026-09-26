//! Fail-hard checks for the public FPGA/MiSTer source package.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn relative_source(base: &Path, line: &str) -> PathBuf {
    base.join(line)
        .canonicalize()
        .unwrap_or_else(|error| panic!("cannot resolve {line}: {error}"))
        .strip_prefix(repo_root())
        .unwrap()
        .to_path_buf()
}

fn required_rtl() -> BTreeSet<PathBuf> {
    let manifest = repo_root().join("scripts/sim.f");
    read("scripts/sim.f")
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("../rtl/"))
        .map(|line| relative_source(manifest.parent().unwrap(), line))
        .filter(|source| source != Path::new("rtl/top/arm7tdmis_no_dft.sv"))
        .collect()
}

fn filelist_sources() -> BTreeSet<PathBuf> {
    let manifest = repo_root().join("fpga/arm7tdmi_mister.f");
    read("fpga/arm7tdmi_mister.f")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("+incdir+"))
        .map(|line| relative_source(manifest.parent().unwrap(), line))
        .collect()
}

fn qip_sources() -> BTreeSet<PathBuf> {
    let pattern = Regex::new(r"\.\./(rtl/[A-Za-z0-9_./-]+\.sv)").unwrap();
    pattern
        .captures_iter(&read("fpga/arm7tdmi_mister.qip"))
        .map(|capture| PathBuf::from(&capture[1]))
        .collect()
}

fn assert_portable(label: &str, text: &str) {
    let absolute_home = Regex::new(r#"(^|[\s"'])/(home|Users|tmp)/"#).unwrap();
    assert!(
        !absolute_home.is_match(text),
        "{label} contains host-specific absolute path"
    );
}

#[test]
fn required_public_files_exist() {
    for relative in [
        "fpga/arm7tdmi_mister.f",
        "fpga/arm7tdmi_mister.qip",
        "fpga/arm7tdmi_mister.qsf",
        "fpga/arm7tdmi_mister.sdc",
        "fpga/arm7tdmis_conformance.qsf",
        "fpga/arm7tdmis_conformance.sdc",
        "fpga/example/arm7tdmi_mister_example_top.sv",
    ] {
        assert!(
            repo_root().join(relative).is_file(),
            "missing public file: {relative}"
        );
    }
}

#[test]
fn filelist_is_complete_and_portable() {
    assert_portable("filelist", &read("fpga/arm7tdmi_mister.f"));
    let sources = filelist_sources();
    assert_eq!(sources, required_rtl());
    for source in sources {
        assert!(
            repo_root().join(&source).is_file(),
            "missing {}",
            source.display()
        );
    }
}

#[test]
fn qip_matches_plain_filelist() {
    let qip = read("fpga/arm7tdmi_mister.qip");
    assert!(qip.contains("$::quartus(qip_path)"));
    assert_portable("QIP", &qip);
    assert_eq!(qip_sources(), required_rtl());
}

#[test]
fn qsf_references_only_public_fragments() {
    let qsf = read("fpga/arm7tdmi_mister.qsf");
    for evidence in [
        "arm7tdmi_mister.qip",
        "arm7tdmi_mister.sdc",
        "example/arm7tdmi_mister_example_top.sv",
        "TOP_LEVEL_ENTITY arm7tdmi_mister_example_top",
    ] {
        assert!(qsf.contains(evidence), "QSF omits {evidence}");
    }
    assert_portable("QSF", &qsf);
}

#[test]
fn conformance_qsf_keeps_raw_feature_complete_top() {
    let qsf = read("fpga/arm7tdmis_conformance.qsf");
    for evidence in [
        "arm7tdmi_mister.qip",
        "arm7tdmis_conformance.sdc",
        "TOP_LEVEL_ENTITY arm7tdmis_top",
    ] {
        assert!(qsf.contains(evidence), "conformance QSF omits {evidence}");
    }
    assert!(!qsf.contains("arm7tdmi_mister_example_top"));
    assert_portable("conformance QSF", &qsf);
}

#[test]
fn example_uses_only_public_wrapper() {
    let example = read("fpga/example/arm7tdmi_mister_example_top.sv");
    for pattern in [
        r"\bmodule\s+arm7tdmi_mister_example_top\b",
        r"\barm7tdmi_mister\b",
    ] {
        assert!(
            Regex::new(pattern).unwrap().is_match(&example),
            "example omits {pattern}"
        );
    }
    for forbidden in [
        ".u_core.",
        ".u_dut.",
        "request_valid_q",
        "response_valid_q",
        "de_q",
        "fetch_pc_q",
    ] {
        assert!(!example.contains(forbidden), "example contains {forbidden}");
    }
}

#[test]
fn public_adapters_are_packaged_and_hierarchy_free() {
    let filelist = filelist_sources();
    let qip = qip_sources();
    for (relative, module) in [
        (
            "rtl/top/arm7tdmi_wishbone_adapter.sv",
            "arm7tdmi_wishbone_adapter",
        ),
        (
            "rtl/top/arm7tdmi_mister_enable_done_adapter.sv",
            "arm7tdmi_mister_enable_done_adapter",
        ),
    ] {
        assert!(
            repo_root().join(relative).is_file(),
            "missing adapter: {relative}"
        );
        let source = read(relative);
        let pattern = Regex::new(&format!(r"\bmodule\s+{}\b", regex::escape(module))).unwrap();
        assert!(
            pattern.is_match(&source),
            "{relative}: missing module {module}"
        );
        assert!(
            filelist.contains(Path::new(relative)),
            "filelist omits {relative}"
        );
        assert!(qip.contains(Path::new(relative)), "QIP omits {relative}");
        for forbidden in [".u_core.", "request_valid_q", "response_valid_q"] {
            assert!(
                !source.contains(forbidden),
                "{relative}: forbidden {forbidden}"
            );
        }
        let private_wrapper =
            Regex::new(r"\b(?:arm7tdmi_mister|arm7tdmis_top)\s*(?:#\s*\()?\s+u_").unwrap();
        assert!(
            !private_wrapper.is_match(&source),
            "{relative}: private wrapper instance"
        );
    }
}

#[test]
fn no_dft_compatibility_wrapper_is_honest_and_excluded() {
    let no_dft = "rtl/top/arm7tdmis_no_dft.sv";
    let misleading = "rtl/top/arm7tdmis_chip.sv";
    assert!(
        !repo_root().join(misleading).exists(),
        "a tied-off scan facade must not be named as a chip/DFT wrapper"
    );
    assert!(
        repo_root().join(no_dft).is_file(),
        "missing named no-DFT wrapper"
    );
    let source = read(no_dft);
    assert!(Regex::new(r"\bmodule\s+arm7tdmis_no_dft\b")
        .unwrap()
        .is_match(&source));
    assert!(!Regex::new(r"\bmodule\s+arm7tdmis_chip\b")
        .unwrap()
        .is_match(&source));
    assert!(source.contains("assign SO = 1'b0;"));
    assert!(!source.contains("scan-flop stitching happens"));
    assert!(!required_rtl().contains(Path::new(no_dft)));
    assert!(!filelist_sources().contains(Path::new(no_dft)));
    assert!(!qip_sources().contains(Path::new(no_dft)));
}
