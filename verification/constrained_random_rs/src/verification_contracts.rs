//! Release-facing VAL-001/002/003 contracts. These replace the Python source
//! checks while the adjacent module tests exercise the Rust behavior itself.

use std::collections::BTreeSet;
use std::fs;

use regex::Regex;
use serde_json::Value;

use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn contains_all(label: &str, contents: &str, markers: &[&str]) {
    for marker in markers {
        assert!(contents.contains(marker), "{label} is missing {marker:?}");
    }
}

fn absent(label: &str, contents: &str, markers: &[&str]) {
    for marker in markers {
        assert!(
            !contents.contains(marker),
            "{label} contains forbidden {marker:?}"
        );
    }
}

fn requirement_block<'a>(tasks: &'a str, start: &str, end: &str) -> &'a str {
    tasks
        .split(start)
        .nth(1)
        .unwrap_or_else(|| panic!("missing task {start}"))
        .split(end)
        .next()
        .unwrap()
}

fn lower_hex(value: &Value, length: usize) -> bool {
    value.as_str().is_some_and(|text| {
        text.len() == length
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[test]
fn public_suite_manifest_pins_redistributable_source_and_signatures() {
    let manifest: Value = serde_json::from_str(&read("verification/public_suites.json")).unwrap();
    assert_eq!(manifest["schema"], "arm7tdmis-public-suites-v1");
    let upstream = &manifest["upstream"];
    assert_eq!(upstream["url"], "https://github.com/jsmolka/gba-suite.git");
    assert!(lower_hex(&upstream["commit"], 40));
    assert!(lower_hex(&upstream["tree"], 40));
    assert_eq!(upstream["license_spdx"], "MIT");
    assert_eq!(upstream["license_path"], "LICENSE");
    assert!(lower_hex(&upstream["license_sha256"], 64));

    let suites = manifest["suites"].as_object().unwrap();
    assert_eq!(
        suites.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["arm", "thumb"])
    );
    let idle_pc = Regex::new(r"^0x08[0-9a-f]{6}$").unwrap();
    for (name, register) in [("arm", 12), ("thumb", 7)] {
        let suite = &suites[name];
        assert!(lower_hex(&suite["upstream_sha256"], 64), "{name}");
        assert!(lower_hex(&suite["patched_sha256"], 64), "{name}");
        assert!(
            idle_pc.is_match(suite["idle_pc"].as_str().unwrap()),
            "{name}"
        );
        assert_eq!(suite["result_register"], register, "{name}");
        let signature = suite["expected_vram_signature"].as_str().unwrap();
        assert!(signature.starts_with("0x") && lower_hex(&Value::from(&signature[2..]), 8));
        assert!(suite["minimum_retirements"].as_u64().unwrap() > 100);
    }
    assert!(manifest["patches"][0]["operation"]
        .as_str()
        .unwrap()
        .contains("zero"));
    assert!(manifest["patches"][0]["range"]
        .as_str()
        .unwrap()
        .contains("0x00000004..0x000000bf"));
}

#[test]
fn public_suite_runner_is_pinned_fail_hard_and_records_provenance() {
    let runner = read("verification/constrained_random_rs/src/public_suite.rs");
    contains_all(
        "public-suite runner",
        &runner,
        &[
            "public_suites.json",
            "git",
            "fetch",
            "--depth=1",
            "rev-parse",
            "sha256",
            "license_sha256",
            "patched_sha256",
            "expected_vram_signature",
            "arm7tdmis-public-suite-v1",
            "wait_timeout",
            "patched_rom",
            "reproducer",
            "[public-suite] PASS",
        ],
    );
    absent(
        "public-suite runner",
        &runner,
        &[
            "ARM Validation Suite",
            "arm7tdmis_instr_pkg",
            "u_dut.u_core",
        ],
    );
}

#[test]
fn public_port_scoreboard_runs_both_manifest_normalized_payloads() {
    let testbench = read("tb/integration/arm7tdmis_public_suite_tb.sv");
    contains_all(
        "public-suite testbench",
        &testbench,
        &[
            "ARM7TDMIS_VERIFICATION",
            "VER_RETIRE_VALID",
            "VER_RETIRE_PC",
            "VER_RETIRE_GPRS",
            "ROM_HEX",
            "IDLE_PC",
            "RESULT_REGISTER",
            "EXPECTED_VRAM_SIGNATURE",
            "REG_DISPSTAT",
            "vblank",
            "boot_words",
            "[public_suite] PASS",
            "$fatal",
        ],
    );
    absent("public-suite testbench", &testbench, &["u_dut.u_core"]);
}

#[test]
fn public_suite_remains_mandatory_documented_and_release_validated() {
    let makefile = read("scripts/Makefile");
    contains_all(
        "Makefile",
        &makefile,
        &[
            "public-suite:",
            "arm7tdmis_public_suite_tb",
            "$(RANDOM_RUST_BIN)) public-suite",
            "public-suite-report.json",
        ],
    );
    assert!(
        read("verification/constrained_random_rs/src/regression.rs").contains("\"public-suite\"")
    );
    assert!(
        read("verification/constrained_random_rs/src/release_evidence.rs")
            .contains("arm7tdmis-public-suite-v1")
    );
    assert!(
        read("verification/constrained_random_rs/src/release_evidence/files/coverage.rs")
            .contains("public-suite-report.json")
    );
    assert!(read(".github/workflows/verification.yml").contains("public_suite/"));
    let tasks = read("TASKS.md");
    assert!(tasks.contains("- [x] **VAL-003:**"));
    contains_all(
        "VAL-003",
        requirement_block(&tasks, "**VAL-003:**", "**VAL-004:**"),
        &[
            "gba-suite",
            "MIT",
            "a7113b67e63f83a9b321696ddd7042ccfad6c881",
            "ARM Validation Suite",
            "not",
        ],
    );
    assert!(read("docs/VERIFICATION.md").contains("Public ARMv4T suite"));
    assert!(read("docs/PROVENANCE.md").contains("jsmolka/gba-suite"));
    absent(
        "limitations",
        &read("docs/LIMITATIONS.md"),
        &["Third-party ARMv4T suites"],
    );
}

#[test]
fn qemu_reference_runner_is_independent_and_fail_hard() {
    let runner = read("verification/constrained_random_rs/src/reference.rs");
    contains_all(
        "QEMU reference",
        &runner,
        &[
            "qemu-system-arm",
            "one-insn-per-tb=on",
            "QEMU emulator version",
            "run_command",
            "expected.hex",
            "program.hex",
            "metadata.json",
            "sha256",
        ],
    );
    absent(
        "QEMU reference",
        &runner,
        &["arm7tdmis_instr_pkg", "arm7tdmis_decoder", "rtl/"],
    );
}

#[test]
fn qemu_program_and_rtl_scoreboard_cross_arm_thumb_and_memory() {
    let program = read("verification/programs/qemu_diff.S").to_lowercase();
    contains_all(
        "QEMU program",
        &program,
        &[
            ".cpu arm7tdmi",
            ".arm",
            ".thumb",
            "mul",
            "ldr",
            "str",
            "ldrh",
            "strh",
            "bx",
            "measure_begin",
            "measure_end",
        ],
    );
    let testbench = read("tb/integration/arm7tdmis_qemu_diff_tb.sv");
    contains_all(
        "QEMU scoreboard",
        &testbench,
        &[
            "ARM7TDMIS_VERIFICATION",
            "VER_RETIRE_VALID",
            "VER_RETIRE_GPRS",
            "VER_RETIRE_CPSR",
            "$readmemh",
            "EXPECTED_EVENTS",
            "expected_memory",
            "[qemu_diff] PASS",
            "$fatal",
        ],
    );
}

#[test]
fn qemu_differential_remains_mandatory_in_local_and_ci_regression() {
    let makefile = read("scripts/Makefile");
    contains_all(
        "Makefile",
        &makefile,
        &[
            "INTEG_TESTS := qemu_diff ",
            "INTEG_EXTRA_FLAGS_qemu_diff := -DARM7TDMIS_VERIFICATION",
            "INTEG_PREP_qemu_diff = $(abspath $(RANDOM_RUST_BIN)) reference",
        ],
    );
    contains_all(
        "CI workflow",
        &read(".github/workflows/verification.yml"),
        &["qemu-system-arm", "clang", "lld"],
    );
    let tasks = read("TASKS.md");
    assert!(tasks.contains("- [x] **VAL-001:**"));
    contains_all(
        "VAL-001",
        requirement_block(&tasks, "**VAL-001:**", "**VAL-002:**"),
        &[
            "verification/constrained_random_rs",
            "arm7tdmis_qemu_diff_tb.sv",
        ],
    );
    contains_all(
        "verification docs",
        &read("docs/VERIFICATION.md"),
        &["QEMU", "integ-qemu_diff"],
    );
}

#[test]
fn seeded_generator_has_independent_reference_and_arm7_policy_lanes() {
    assert!(repo_root()
        .join("verification/constrained_random_rs/Cargo.toml")
        .is_file());
    let campaign = read("verification/constrained_random_rs/src/campaign.rs");
    let generation = read("verification/constrained_random_rs/src/generation.rs");
    let random = read("verification/constrained_random_rs/src/random.rs");
    let qemu = read("verification/constrained_random_rs/src/qemu.rs");
    contains_all(
        "campaign",
        &campaign,
        &[
            "const SCHEMA: &str = \"arm7tdmis-constrained-random-v1\"",
            "qemu-system-arm",
            "one-insn-per-tb=on",
            "rust_engine_sha256",
            "reproducer",
            "sha256",
            "--seeds",
            "--instructions",
        ],
    );
    contains_all(
        "generation",
        &generation,
        &[
            "PythonRandom",
            ".cpu arm7tdmi",
            ".arm",
            ".thumb",
            "MODE_FIQ",
            "MODE_IRQ",
            "MODE_SUPERVISOR",
            "MODE_ABORT",
            "MODE_UNDEFINED",
            "svc",
            "undefined",
            "little",
            "big",
            "unaligned",
            "dependency",
            "permitted_memory",
        ],
    );
    assert!(random.contains("init_genrand"));
    assert!(qemu.contains("PSR_COMPARE_MASK"));
    absent(
        "independent oracle",
        &(campaign + &generation + &qemu),
        &["arm7tdmis_instr_pkg", "arm7tdmis_decoder", "rtl/"],
    );
}

#[test]
fn constrained_random_scoreboards_compare_retirement_and_memory_without_peeking() {
    let differential = read("tb/integration/arm7tdmis_random_diff_tb.sv");
    let policy = read("tb/integration/arm7tdmis_random_policy_tb.sv");
    contains_all(
        "differential scoreboard",
        &differential,
        &[
            "ARM7TDMIS_VERIFICATION",
            "VER_RETIRE_VALID",
            "VER_RETIRE_EXCEPTION_VALID",
            "VER_RETIRE_GPRS",
            "VER_RETIRE_CPSR",
            "EXPECTED_EVENTS",
            "EXPECTED_HEX",
            "active_physical_index",
            "PERMITTED_MEMORY_HEX",
            "$readmemh",
            "[random_diff] PASS",
            "$fatal",
        ],
    );
    contains_all(
        "policy scoreboard",
        &policy,
        &[
            "BIG_ENDIAN",
            "EXPECTED_MEMORY_HEX",
            "EXPECTED_WORDS",
            "$readmemh",
            "[random_policy] PASS",
            "$fatal",
        ],
    );
    absent("differential scoreboard", &differential, &["u_dut.u_core"]);
    absent("policy scoreboard", &policy, &["u_dut.u_core"]);
}

#[test]
fn constrained_random_release_campaign_remains_long_reproducible_and_mandatory() {
    let makefile = read("scripts/Makefile");
    contains_all(
        "Makefile",
        &makefile,
        &[
            "RANDOM_SEEDS ?= 32",
            "RANDOM_INSTRUCTIONS ?= 256",
            "random-validation:",
            "arm7tdmis_random_diff_tb",
            "arm7tdmis_random_policy_tb",
            "arm7-constrained-random",
            "RANDOM_RUST_BIN",
            "cargo build --release --locked",
        ],
    );
    contains_all(
        "regression",
        &read("verification/constrained_random_rs/src/regression.rs"),
        &["\"random-validation\"", "\"random-validation-quick\""],
    );
    assert!(
        read("verification/constrained_random_rs/src/release_evidence.rs")
            .contains("arm7tdmis-constrained-random-v1")
    );
    assert!(
        read("verification/constrained_random_rs/src/release_evidence/files/random.rs")
            .contains("constrained-random-report.json")
    );
    assert!(read(".github/workflows/verification.yml").contains("random-validation-quick"));
    let tasks = read("TASKS.md");
    assert!(tasks.contains("- [x] **VAL-002:**"));
    contains_all(
        "VAL-002",
        requirement_block(&tasks, "**VAL-002:**", "**VAL-003:**"),
        &[
            "32",
            "256",
            "8,192",
            "QEMU",
            "ARM926",
            "big-endian",
            "unaligned",
            "reproducer",
        ],
    );
    contains_all(
        "verification docs",
        &read("docs/VERIFICATION.md"),
        &["Constrained-random", "arm7tdmis-constrained-random-v1"],
    );
    absent(
        "limitations",
        &read("docs/LIMITATIONS.md"),
        &["Constrained-random generation"],
    );
}
