//! Public documentation assertions formerly kept in a Python unittest.

use std::fs;

use regex::Regex;

use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn contains_all(label: &str, contents: &str, phrases: &[&str]) {
    for phrase in phrases {
        assert!(contents.contains(phrase), "{label} is missing {phrase:?}");
    }
}

fn absent(label: &str, contents: &str, phrases: &[&str]) {
    for phrase in phrases {
        assert!(
            !contents.contains(phrase),
            "{label} contains stale claim {phrase:?}"
        );
    }
}

#[test]
fn open_blocker_summary_matches_the_canonical_ledger() {
    let tasks = read("TASKS.md");
    let limitations = read("docs/LIMITATIONS.md");
    let unchecked = Regex::new(r"(?m)^- \[ \] \*\*([A-Z]+-\d+):\*\*").unwrap();
    let ids = unchecked
        .captures_iter(&tasks)
        .map(|capture| capture[1].to_owned())
        .collect::<Vec<_>>();
    assert!(!ids.is_empty(), "TASKS.md has no unchecked requirement IDs");
    for id in ids {
        assert!(limitations.contains(&id), "docs/LIMITATIONS.md omits {id}");
    }

    let maintained = [
        "README.md",
        "AGENTS.md",
        "docs/LIMITATIONS.md",
        "docs/SUPPORT.md",
        "docs/TABLE7_MATRIX.md",
    ]
    .map(read)
    .join("\n");
    absent(
        "maintained status",
        &maintained,
        &[
            "formal closure remains",
            "formal validation",
            "Formal proof and independent review remain open",
            "Formal and FPGA release gates remain open",
            "Yosys/SymbiYosys are not currently installed",
            "Remaining VAL items require formal",
        ],
    );
}

#[test]
fn public_status_does_not_repeat_known_stale_claims() {
    let public = read("README.md") + &read("AGENTS.md");
    absent(
        "public status",
        &public,
        &[
            "accepted-operation and busy/data protocols do not",
            "complete exception/abort release gate remains open",
            "broader exception/abort release gate remains open",
            "§31.6 remains the sign-off ledger",
            "No Quartus on the build box yet",
            "Known open categories still include exact debug-pin sampling",
            "synthesis/timing evidence, and MiSTer/PocketStation packaging",
            "per-module SystemVerilog testbenches (10 tests)",
            "15 `make integ` benches",
            "The goal is a **cycle-accurate, synthesizable**",
        ],
    );
}

#[test]
fn release_status_uses_the_canonical_ledger() {
    let readme = read("README.md");
    let agents = read("AGENTS.md");
    let tasks = read("TASKS.md");
    for (label, contents) in [("README", &readme), ("AGENTS", &agents)] {
        contains_all(label, contents, &["Not sign-off ready", "TASKS.md"]);
    }
    for status in ["VERIFIED", "IMPLEMENTED-UNVERIFIED", "PARTIAL"] {
        assert!(tasks.contains(status), "TASKS.md omits {status}");
        assert!(readme.contains(status), "README.md omits {status}");
    }
}

#[test]
fn corrected_architectural_contracts_remain_documented() {
    for (path, phrases) in [
        (
            "docs/EXCEPTIONS.md",
            &[
                "DABT",
                "FIQ > IRQ > PABT > UNDEF > SWI",
                "source instruction address plus 4",
                "source instruction address plus 8",
                "LDM/STM Data Abort completion",
                "LDM Base Updated result",
                "STM writeback and stores",
            ][..],
        ),
        (
            "docs/PSR.md",
            &[
                "There is no Q flag in ARMv4T",
                "leaves the complete CPSR unchanged",
            ],
        ),
        (
            "docs/PIPELINE.md",
            &["instruction PC `+ 12`", "STM has no I cycle"],
        ),
        (
            "docs/COPROCESSOR.md",
            &[
                "It does not contain CP15",
                "Internal CP14 accepts only these exact",
                "Present but busy",
                "Base Updated abort model",
            ],
        ),
        (
            "docs/RAW_BUS.md",
            &[
                "Little endian (`CFGBIGEND=0`)",
                "Big endian (`CFGBIGEND=1`)",
            ],
        ),
        (
            "docs/DEBUG.md",
            &[
                "monitor-mode restrictions are enforced fail-closed",
                "CP14 DCC and Debug Abort Status",
            ],
        ),
        (
            "docs/TRACE.md",
            &[
                "It does not implement an Embedded Trace Macrocell",
                "Table 6-1 adapter",
            ],
        ),
        (
            "docs/INTEGRATION.md",
            &["arm7tdmis_no_dft", "does not provide a scan-insertion flow"],
        ),
    ] {
        contains_all(path, &read(path), phrases);
    }
}

#[test]
fn registered_test_inventory_is_not_hard_coded_in_readme() {
    let readme = read("README.md");
    absent("README.md", &readme, &["## Current test inventory"]);
    contains_all(
        "README.md",
        &readme,
        &["scripts/Makefile", "machine-readable"],
    );
}

#[test]
fn verified_comments_do_not_preserve_obsolete_milestone_language() {
    for (path, stale) in [
        (
            "tb/integration/arm7tdmis_memory.sv",
            "unused at this scaffold level",
        ),
        (
            "rtl/jtag/arm7tdmis_jtag_tap.sv",
            "off-chip Multi-ICE synchronizer is deferred to a later milestone",
        ),
    ] {
        absent(path, &read(path), &[stale]);
    }
}
