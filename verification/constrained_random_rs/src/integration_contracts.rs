//! Contracts for the pinned compiler-program lane and the portable SoC example.

use std::fs;

use crate::campaign::repo_root;

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn contains_all(label: &str, source: &str, markers: &[&str]) {
    for marker in markers {
        assert!(source.contains(marker), "{label} is missing {marker:?}");
    }
}

fn absent(label: &str, source: &str, markers: &[&str]) {
    for marker in markers {
        assert!(
            !source.contains(marker),
            "{label} contains forbidden {marker:?}"
        );
    }
}

fn task_block<'a>(tasks: &'a str, start: &str, end: &str) -> &'a str {
    tasks
        .split(start)
        .nth(1)
        .unwrap_or_else(|| panic!("missing task {start}"))
        .split(end)
        .next()
        .unwrap()
}

#[test]
fn compiler_toolchain_is_versioned_and_hash_verified() {
    let installer = read("verification/constrained_random_rs/src/arm_toolchain.rs")
        + &read("verification/constrained_random_rs/src/pinned_archive.rs");
    contains_all(
        "Arm toolchain installer",
        &installer,
        &[
            "14.3.rel1",
            "arm-none-eabi",
            "8f6903f8ceb084d9227b9ef991490413014d991874a1e34074443c2a72b14dbd",
            "sha256",
            "curl",
            "tar",
            "metadata.json",
        ],
    );
}

#[test]
fn compiler_program_sources_build_arm_thumb_and_bidirectional_interworking() {
    let root = repo_root();
    for file in ["startup.S", "arm_program.c", "thumb_program.c", "linker.ld"] {
        assert!(
            root.join("verification/compiler").join(file).is_file(),
            "missing {file}"
        );
    }
    assert!(read("verification/compiler/arm_program.c").contains("thumb_accumulate"));
    assert!(read("verification/compiler/thumb_program.c").contains("arm_mix"));
    contains_all(
        "startup",
        &read("verification/compiler/startup.S"),
        &[".arm", "arm_main"],
    );
    contains_all(
        "compiler builder",
        &read("verification/constrained_random_rs/src/compiler_program.rs"),
        &[
            "-march=armv4t",
            "-marm",
            "-mthumb",
            "-mthumb-interwork",
            "-ffreestanding",
            "-nostdlib",
            "program.hex",
            "disassembly.txt",
        ],
    );
}

#[test]
fn compiler_program_execution_is_fail_hard_and_always_regressed() {
    contains_all(
        "compiler testbench",
        &read("tb/integration/arm7tdmis_compiler_tb.sv"),
        &[
            "$readmemh",
            "VER_RETIRE_THUMB",
            "saw_arm",
            "saw_thumb",
            "MAILBOX_DONE",
            "[compiler] PASS",
            "$fatal",
        ],
    );
    contains_all(
        "Makefile",
        &read("scripts/Makefile"),
        &[
            "compiler",
            "arm-toolchain-install",
            "compiler-program-build",
        ],
    );
    contains_all(
        "regression",
        &read("verification/constrained_random_rs/src/regression.rs"),
        &["QUICK_INTEGRATIONS", "\"compiler\""],
    );
    contains_all(
        "release evidence",
        &read("verification/constrained_random_rs/src/release_evidence/files.rs"),
        &[
            "\"integration-compiler\"",
            "\"compiler-program metadata is missing\"",
        ],
    );
    contains_all(
        "workflow",
        &read(".github/workflows/verification.yml"),
        &["curl", "xz-utils"],
    );
    let tasks = read("TASKS.md");
    assert!(tasks.contains("- [x] **VAL-009:**"));
    contains_all(
        "VAL-009",
        task_block(&tasks, "**VAL-009:**", "**VAL-010:**"),
        &["arm7tdmis_compiler_tb.sv", "14.3.Rel1"],
    );
}

#[test]
fn generic_soc_is_synthesizable_and_uses_only_public_cpu_api() {
    let source = read("examples/generic_soc/arm7tdmi_generic_soc.sv");
    contains_all(
        "generic SoC",
        &source,
        &[
            "module arm7tdmi_generic_soc",
            "arm7tdmi_mister",
            "ROM_BASE",
            "RAM_BASE",
            "TIMER_BASE",
            "UART_BASE",
            "TEST_BASE",
            "TEST_STATUS",
            "TEST_SIGNATURE",
        ],
    );
    absent(
        "generic SoC",
        &source,
        &["u_cpu.", "`include", "initial begin", "$readmem", "#"],
    );
}

#[test]
fn generic_soc_test_proves_ram_uart_timer_interrupt_and_backpressure() {
    contains_all(
        "generic SoC testbench",
        &read("examples/generic_soc/arm7tdmi_generic_soc_tb.sv"),
        &[
            "UART_TX_READY",
            "UART_TX_VALID",
            "8'h47",
            "8'h49",
            "8'h50",
            "saw_timer_irq",
            "saw_test",
            "TEST_STATUS",
            "TEST_SIGNATURE",
            "32'ha7d1_c0de",
            "CPU_CE",
            "backpressure",
            "$fatal",
            "[generic_soc] PASS",
        ],
    );
}

#[test]
fn generic_soc_has_two_open_source_frontends_and_mandatory_simulation() {
    let makefile = read("scripts/Makefile");
    contains_all(
        "Makefile",
        &makefile,
        &[
            "lint-generic-soc:",
            "lint-generic-soc-slang:",
            "generic-soc-rom-check:",
            "sim-generic-soc:",
            "generic-soc-rom-check: $(RANDOM_RUST_BIN)",
            "$(abspath $(RANDOM_RUST_BIN)) generic-soc-rom-check",
        ],
    );
    contains_all(
        "regression",
        &read("verification/constrained_random_rs/src/regression.rs"),
        &[
            "lint-generic-soc-slang",
            "generic-soc-rom-check",
            "sim-generic-soc",
        ],
    );
    contains_all(
        "workflow",
        &read(".github/workflows/verification.yml"),
        &[
            "v11.0/slang-linux-x86_64.tar.gz",
            "951a170e10e25e54c91565030acfdfc11c3226714ebf225a18ad4166a898d8a4",
        ],
    );
}

#[test]
fn generic_soc_documentation_freezes_memory_map_and_mist_010() {
    let documentation = read("docs/GENERIC_SOC.md");
    contains_all(
        "generic SoC documentation",
        &documentation,
        &[
            "0x0000_0000",
            "0x1000_0000",
            "0x2000_0000",
            "0x2000_1000",
            "0x2000_2000",
            "UART_TX_READY",
            "0xa7d1_c0de",
        ],
    );
    assert!(documentation.to_lowercase().contains("timer"));
    assert!(read("TASKS.md").contains("- [x] **MIST-010:**"));
}

#[test]
fn generic_soc_armv4t_program_covers_board_smoke_paths() {
    let program = read("examples/generic_soc/program.S");
    contains_all(
        "generic SoC program",
        &program,
        &[
            ".arch armv4t",
            "ldrb",
            "strb",
            "strh",
            "ldrh",
            "ldrsb",
            "umull",
            "stmdb",
            "ldmia",
            "swp",
            "bx",
            "irq_handler",
            "thumb_test",
            "0xa7d1c0de",
            "0x50415353",
            "0x4641494c",
        ],
    );
    absent("generic SoC program", &program.to_lowercase(), &["blx"]);
    assert!(read("examples/generic_soc/linker.ld").contains("SIZEOF(.text) <= 0x400"));
}
