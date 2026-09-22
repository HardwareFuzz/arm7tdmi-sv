# Build / lint / sim flow for arm7tdmi-sv. Run from this directory:
#   make lint            # Verilator --lint-only -Wall on the RTL filelist
#   make lint-tb         # same, but elaborate the integration testbench top
#   make sim             # build the integration testbench binary
#   make run             # build and run; produces cycle.csv + waves.fst
#   make wave            # open the FST in GTKWave
#   make unit-regfile    # build + run the regfile unit test
#   make unit-psr        # build + run the PSR unit test
#   make unit            # run every unit test
#   make clean           # remove obj_dir + simulation artifacts
# or from the repo root: make -C scripts <target>

VERILATOR ?= verilator
VERILATOR_COVERAGE ?= verilator_coverage
LOCAL_SLANG := $(firstword $(wildcard ../.tools/slang \
	../.tools/slang-11.0/slang))
SLANG     ?= $(if $(LOCAL_SLANG),$(LOCAL_SLANG),slang)
GTKWAVE   ?= gtkwave
PYTHON    ?= python3
CLANG     ?= clang
LLVM_OBJCOPY ?= llvm-objcopy
LLVM_NM   ?= llvm-nm
QEMU_SYSTEM_ARM ?= qemu-system-arm
QUARTUS_MAP ?= quartus_map
QUARTUS_FIT ?= quartus_fit
QUARTUS_EDA ?= quartus_eda
QUARTUS_SH  ?= quartus_sh
QUARTUS_POW ?= quartus_pow

SIM_F     := sim.f
TB_F      := tb.f
FPGA_F    := ../fpga/arm7tdmi_mister.f
FPGA_EXAMPLE := ../fpga/example/arm7tdmi_mister_example_top.sv
GENERIC_SOC := ../examples/generic_soc/arm7tdmi_generic_soc.sv
GENERIC_SOC_TB := ../examples/generic_soc/arm7tdmi_generic_soc_tb.sv
GENERIC_SOC_ROM_OUTPUT := ../reports/generated/generic-soc-rom
OPTION_PROJECTS := arm7tdmi_option_none arm7tdmi_option_debug \
	arm7tdmi_option_coprocessor arm7tdmi_option_both

TOP_RTL   := arm7tdmis_top
TOP_TB    := arm7tdmis_tb_top

# Verilator warnings are fatal by default. The only global RTL exception is
# UNUSEDPARAM because each elaborated top consumes a different subset of the
# shared public package API. Integration builds additionally allow the audited
# mixed-use testbench reset net. See docs/WARNING_POLICY.md.
VFLAGS_COMMON := -Wall -Wno-UNUSEDPARAM --assert --timescale 1ns/1ps
VFLAGS_LINT   := $(VFLAGS_COMMON)
VFLAGS_BUILD  := $(VFLAGS_COMMON) --binary --trace-fst -j 0
VFLAGS_COVERAGE := $(VFLAGS_COMMON) --binary --coverage -j 0

OBJ_DIR        := obj_dir
TB_BIN         := $(OBJ_DIR)/V$(TOP_TB)
RUN_ARTIFACTS  := cycle.csv waves.fst
COVERAGE_OBJ_DIR := $(OBJ_DIR)/coverage
COVERAGE_REPORT_DIR := ../reports/generated/coverage
QEMU_DIFF_DIR := ../reports/generated/qemu_diff
COMPILER_DIR := ../reports/generated/compiler
TRACEABILITY_REPORT := ../reports/generated/traceability-report.json
TRM_COVERAGE_REPORT := ../reports/generated/trm-coverage-report.json
AC_TIMING_REPORT := ../reports/generated/ac-timing-report.json
INDEPENDENT_LINT_REPORT := ../reports/generated/independent-lint.json
CDC_RDC_REPORT := ../reports/generated/cdc-rdc.json
POSTFIT_REPORT := ../reports/generated/postfit-report.json
MISTER_FRAMEWORK_REPORT := ../reports/generated/mister-framework-report.json
MISTER_FRAMEWORK_ARTIFACT_DIR := ../reports/generated/mister-framework
RANDOM_SEEDS ?= 32
RANDOM_INSTRUCTIONS ?= 256
RANDOM_OBJ_DIR := $(OBJ_DIR)/constrained_random
RANDOM_DIFF_BIN := $(RANDOM_OBJ_DIR)/diff/Varm7tdmis_random_diff_tb
RANDOM_POLICY_BIN := $(RANDOM_OBJ_DIR)/policy/Varm7tdmis_random_policy_tb
RANDOM_REPORT := ../reports/generated/constrained-random-report.json
STANDALONE_JOBS ?= 8
STANDALONE_DIR := $(OBJ_DIR)/standalone
STANDALONE_BIN := $(STANDALONE_DIR)/Varm7tdmis_standalone_tb
RANDOM_EVENT_SEEDS ?= 32
RANDOM_EVENT_OBJ_DIR := $(OBJ_DIR)/random_events
RANDOM_EVENT_BIN := \
	$(RANDOM_EVENT_OBJ_DIR)/Varm7tdmis_random_event_matrix_tb
RANDOM_EVENT_REPORT := ../reports/generated/random-events-report.json
PUBLIC_SUITE_OBJ_DIR := $(OBJ_DIR)/public_suite
PUBLIC_SUITE_BIN := $(PUBLIC_SUITE_OBJ_DIR)/Varm7tdmis_public_suite_tb
PUBLIC_SUITE_REPORT := ../reports/generated/public-suite-report.json
PUBLIC_SUITE_ARGS ?=
SOAK_SEEDS ?= 256
SOAK_OBJ_DIR := $(OBJ_DIR)/soak
SOAK_BIN := $(SOAK_OBJ_DIR)/Varm7tdmis_mister_wrapper_tb
SOAK_REPORT := ../reports/generated/soak-report.json
FORMAL_REPORT := ../reports/generated/formal-report.json
FORMAL_JOBS ?= 2
# Full-regression aggregate gates must consume the report currently being
# written by the harness. The harness overrides this for alternate repo-local
# report paths; direct Make invocations retain the release default.
REGRESSION_REPORT ?= reports/generated/regression.json
ARM_TOOLCHAIN_ROOT := ../.tools
ARM_TOOLCHAIN := \
	$(ARM_TOOLCHAIN_ROOT)/arm-gnu-toolchain-14.3.rel1-x86_64-arm-none-eabi

UNIT_TESTS := regfile regfile_banking psr psr_banking psr_policy reset_state_policy reset_sync shifter alu multiplier condition decoder thumb_decoder reserved_decode unpredictable_decode jtag_tap jtag_idcode_config sync_debug_port ice_rt ice_watchpoint dcc etm7_adapter adapters

INTEG_TESTS := qemu_diff compiler umull umlal multiply_matrix thumb_multiply_bus thumb_reg_shift_bus cp15_undef cp14_decode cp14_dcc cp14_r15 cp_pipeline_follow cp_cdp_protocol cp_reg_transfer cp_reg_r15 cp_ldc_stc cp_ldc_stc_abort cp_address_wrap_policy cp_erratum14 cp_erratum15 cp_busy_interrupt cp_busy_dbgrq cpntrans debug_status debug_control_sync debug_reserved_regs debug_software_breakpoint debug_watchpoint_completion debug_watchpoint_priority debug_watchpoint_pabt_sequence debug_breakpoint_pabt debug_breakpoint_interrupt debug_dbgrq_exception debug_pc_modify_dbgrq debug_breakpoint_flush debug_breakpoint_execute debug_consecutive_breakpoints debug_multicycle_breakpoints debug_exception_breakpoint_sequence debug_external_break debug_inject_handshake debug_inject_matrix debug_dbgen_gating debug_dbgen_sources debug_entry_cause debug_system_speed debug_system_speed_abort debug_register_scan debug_pc_capture debug_pc_resume debug_monitor_mode etm_trace_matrix mister_wrapper mister_savestate mister_cdc_reset mister_profiles mister_dma_arbitration retire_interface abort_inactive abort_clken endian_data endian_thumb translated_ls translated_ls_matrix unaligned_access_matrix ldr_pc_unaligned_policy ldr_pc_bus thumb_word_unaligned_policy thumb_halfword_policy single_ls_matrix extra_ls_matrix single_ls_policy block_ls_matrix block_ls_policy register_banking_matrix thumb_block_policy swp_policy swp_bus_matrix cond_suppress cond_fail_matrix sequence_dependencies thumb_bl_boundary thumb_bl_pair_policy thumb_bx_pc_policy address_wrap_policy dp_pc_no_spsr_policy invalid_spsr_return_policy unpredictable_runtime unpredictable_trap reset_clken reset_multicycle_matrix fetch_sequence table7_core_phase_matrix bus_burst dmore_matrix abort irq fiq interrupt_sampling_matrix interrupt_latency exception_priority dabt_fiq arm_exception_lr thumb_exception_lr erratum12_thumb_dabt_lr exception_bus_matrix exception_return_matrix ldm_pc pabt pabt_pipeline erratum2_exception_priority pabt_debug_flush ldm_abort ldm_abort_base_list stm_abort stm_base_list ls_abort_modes swp_read_abort cycles swi undef reserved_execute thumb bx_interwork swpb_data flags_preserve psr_policy pc_operands pc_write_alignment mull_flags

ERRATA_TESTS := debug_consecutive_breakpoints erratum2_exception_priority \
	debug_dbgrq_exception debug_exception_breakpoint_sequence \
	debug_multicycle_breakpoints debug_watchpoint_pabt_sequence \
	debug_watchpoint_priority debug_breakpoint_execute debug_system_speed \
	cp14_dcc etm_trace_matrix cond_fail_matrix pabt_pipeline \
	erratum12_thumb_dabt_lr debug_pc_modify_dbgrq cp_erratum14 cp_erratum15

# This is a deliberately named measured subset, not functional-coverage
# closure. It crosses retirement, dependency/condition/flags, exception entry,
# bus transfer, and debug execution. The JSON report records the exact list.
COVERAGE_TESTS := retire_interface sequence_dependencies arm_exception_lr debug_system_speed
COVERAGE_RAW_FILES := $(addsuffix .dat,$(addprefix $(COVERAGE_REPORT_DIR)/,$(COVERAGE_TESTS)))

INTEG_COMMON_SRCS := \
	../verification/arm7tdmis_raw_bus_checker.sv \
	../tb/integration/arm7tdmis_jtag_tb_pkg.sv \
	../tb/integration/arm7tdmis_memory.sv \
	../tb/integration/arm7tdmis_assertions.sv \
	../tb/integration/arm7tdmis_test_fixture.sv \
	../tb/integration/arm7tdmis_raw_bus_bind.sv

# VER-009 instrumentation is absent from production/synthesis builds. Tests
# that consume the public retirement contract opt into those ports explicitly.
INTEG_EXTRA_FLAGS_retire_interface := -DARM7TDMIS_VERIFICATION
INTEG_EXTRA_FLAGS_mister_savestate := -DARM7TDMIS_SAVE_STATE
INTEG_EXTRA_FLAGS_qemu_diff := -DARM7TDMIS_VERIFICATION -I$(QEMU_DIFF_DIR)
INTEG_PREP_qemu_diff = $(PYTHON) ../verification/qemu_armv4t_reference.py \
	--output $(QEMU_DIFF_DIR) --clang $(CLANG) \
	--objcopy $(LLVM_OBJCOPY) --nm $(LLVM_NM) --qemu $(QEMU_SYSTEM_ARM)
INTEG_RUN_ARGS_qemu_diff = \
	+PROGRAM_HEX=$(abspath $(QEMU_DIFF_DIR)/program.hex) \
	+EXPECTED_HEX=$(abspath $(QEMU_DIFF_DIR)/expected.hex)
INTEG_EXTRA_FLAGS_compiler := -DARM7TDMIS_VERIFICATION
INTEG_PREP_compiler = $(PYTHON) ../scripts/install_arm_toolchain.py \
	--root $(ARM_TOOLCHAIN_ROOT) && \
	$(PYTHON) ../scripts/build_compiler_program.py \
	--toolchain $(ARM_TOOLCHAIN) --output $(COMPILER_DIR)
INTEG_RUN_ARGS_compiler = \
	+PROGRAM_HEX=$(abspath $(COMPILER_DIR)/program.hex)

.PHONY: all lint lint-mister lint-example \
        lint-generic-soc lint-generic-soc-slang generic-soc-rom-check \
        generic-soc-rom-artifacts lint-independent \
        cdc-rdc fpga-quality sim-generic-soc lint-tb \
        quartus-analysis quartus-compile \
        quartus-conformance-analysis quartus-conformance-compile \
        quartus-option-characterization postfit-sim mister-framework \
        sim run wave clean help regress regress-quick regress-ci \
        coverage release-evidence \
        formal \
        traceability trm-coverage ac-timing random-validation-build random-validation \
        random-validation-quick random-events-build random-events \
        public-suite-build public-suite \
        table7-cross functional-coverage \
        soak-build soak \
        harness-unit harness-self-test raw-checker-self-test mutation-self-test \
        $(addprefix unit-, $(UNIT_TESTS)) unit \
        $(addprefix integ-, $(INTEG_TESTS)) integ errata

all: lint

lint:
	$(VERILATOR) --lint-only $(VFLAGS_LINT) --top-module $(TOP_RTL) -f $(SIM_F)

lint-mister:
	$(VERILATOR) --lint-only $(VFLAGS_LINT) \
	    --top-module arm7tdmi_mister -f $(SIM_F)

lint-example:
	$(VERILATOR) --lint-only $(VFLAGS_LINT) \
	    --top-module arm7tdmi_mister_example_top \
	    -f $(FPGA_F) $(FPGA_EXAMPLE)

lint-generic-soc:
	$(VERILATOR) --lint-only $(VFLAGS_LINT) \
	    --top-module arm7tdmi_generic_soc \
	    -f $(SIM_F) $(GENERIC_SOC)

lint-generic-soc-slang:
	$(SLANG) --version
	$(SLANG) --allow-use-before-declare \
	    --top arm7tdmi_generic_soc -f $(SIM_F) $(GENERIC_SOC)

generic-soc-rom-check:
	$(PYTHON) install_arm_toolchain.py --root $(ARM_TOOLCHAIN_ROOT)
	$(PYTHON) check_generic_soc_rom.py --toolchain $(ARM_TOOLCHAIN)

generic-soc-rom-artifacts:
	$(PYTHON) install_arm_toolchain.py --root $(ARM_TOOLCHAIN_ROOT)
	$(PYTHON) check_generic_soc_rom.py --toolchain $(ARM_TOOLCHAIN) \
	    --output $(GENERIC_SOC_ROM_OUTPUT)

lint-independent:
	PYTHONPATH=. $(PYTHON) independent_lint.py \
	    --slang $(SLANG) --output $(INDEPENDENT_LINT_REPORT)

cdc-rdc:
	PYTHONPATH=. $(PYTHON) cdc_rdc_check.py --check \
	    --output $(CDC_RDC_REPORT)

fpga-quality: lint-independent cdc-rdc

sim-generic-soc:
	mkdir -p $(OBJ_DIR)
	$(VERILATOR) $(VFLAGS_BUILD) \
	    --top-module arm7tdmi_generic_soc_tb \
	    --Mdir $(OBJ_DIR)/generic_soc \
	    -f $(SIM_F) $(GENERIC_SOC) $(GENERIC_SOC_TB)
	./$(OBJ_DIR)/generic_soc/Varm7tdmi_generic_soc_tb

# Exercise the checked QSF/QIP and synthesize/elaborate every public-wrapper
# dependency with the MiSTer-mandated Quartus 17 SystemVerilog frontend.
quartus-analysis:
	cd ../fpga && $(QUARTUS_MAP) --analysis_and_elaboration \
	    --read_settings_files=on --write_settings_files=off \
	    arm7tdmi_mister -c arm7tdmi_mister

# Complete selected-device flow: synthesis, fit, TimeQuest, and assembly.
quartus-compile:
	cd ../fpga && $(QUARTUS_SH) --flow compile arm7tdmi_mister
	cd ../fpga && $(QUARTUS_POW) arm7tdmi_mister \
	    --read_settings_files=on --write_settings_files=off \
	    --use_vectorless_estimation=on
	PYTHONPATH=. $(PYTHON) quartus_report_check.py ../fpga/output_files

quartus-conformance-analysis:
	cd ../fpga && $(QUARTUS_MAP) --analysis_and_elaboration \
	    --read_settings_files=on --write_settings_files=off \
	    arm7tdmis_conformance -c arm7tdmis_conformance

quartus-conformance-compile:
	cd ../fpga && $(QUARTUS_SH) --flow compile arm7tdmis_conformance
	cd ../fpga && $(QUARTUS_POW) arm7tdmis_conformance \
	    --read_settings_files=on --write_settings_files=off \
	    --use_vectorless_estimation=on
	PYTHONPATH=. $(PYTHON) quartus_report_check.py \
	    --project arm7tdmis_conformance \
	    --top arm7tdmis_top \
	    --max-alm 7500 \
	    --max-register 6000 \
	    --max-dsp 8 \
	    --max-memory-bit 0 \
	    ../fpga/conformance_output_files

quartus-option-characterization:
	@set -eu; \
	for project in $(OPTION_PROJECTS); do \
	    cd ../fpga; \
	    $(QUARTUS_SH) --flow compile $$project; \
	    $(QUARTUS_POW) $$project \
	        --read_settings_files=on --write_settings_files=off \
	        --use_vectorless_estimation=on; \
	    cd ../scripts; \
	done
	PYTHONPATH=. $(PYTHON) quartus_option_characterization.py

# FPGA-006 uses temporary little- and big-endian synthesis projects so the
# production QSFs are never rewritten by the EDA netlist writer.
postfit-sim:
	PYTHONPATH=. $(PYTHON) postfit_sim.py \
	    --output $(POSTFIT_REPORT) \
	    --quartus-map $(QUARTUS_MAP) \
	    --quartus-fit $(QUARTUS_FIT) \
	    --quartus-eda $(QUARTUS_EDA) \
	    --verilator $(VERILATOR)

mister-framework:
	PYTHONPATH=. $(PYTHON) mister_framework_build.py \
	    --quartus-sh $(QUARTUS_SH) \
	    --output $(MISTER_FRAMEWORK_REPORT) \
	    --artifact-dir $(MISTER_FRAMEWORK_ARTIFACT_DIR)

lint-tb:
	$(VERILATOR) --lint-only $(VFLAGS_LINT) --top-module $(TOP_TB) -f $(SIM_F) -f $(TB_F)

RTL_SRCS := $(shell find ../rtl -name '*.sv')
TB_SRCS  := $(shell find ../tb/integration -name '*.sv')

sim: $(TB_BIN)

$(TB_BIN): $(SIM_F) $(TB_F) $(RTL_SRCS) $(TB_SRCS)
	$(VERILATOR) $(VFLAGS_BUILD) --top-module $(TOP_TB) -f $(SIM_F) -f $(TB_F)

run: sim
	./$(TB_BIN)

wave: run
	$(GTKWAVE) waves.fst &

# Per-unit-test recipe: each test has its top module named `<name>_tb` in
# tb/unit/<name>_tb.sv, builds into obj_dir/<name>/V<name>_tb, and runs
# from that path. Generated explicitly per test (a `unit-%` pattern rule
# doesn't fire reliably for phony targets).
define UNIT_RULE
unit-$(1):
	mkdir -p $$(OBJ_DIR)
	$$(VERILATOR) $$(VFLAGS_BUILD) --top-module $(1)_tb \
	    --Mdir $$(OBJ_DIR)/$(1) -f $$(SIM_F) ../tb/unit/$(1)_tb.sv
	./$$(OBJ_DIR)/$(1)/V$(1)_tb
endef

$(foreach t,$(UNIT_TESTS),$(eval $(call UNIT_RULE,$(t))))

unit: $(addprefix unit-, $(UNIT_TESTS))

# Per-integration-test recipe. Each test has its top module named
# `arm7tdmis_<name>_tb` in tb/integration/arm7tdmis_<name>_tb.sv and pulls
# the shared integration TB infrastructure (memory model, cycle logger,
# assertions) via tb.f.
define INTEG_RULE
integ-$(1):
	mkdir -p $$(OBJ_DIR)
	$$(INTEG_PREP_$(1))
	$$(VERILATOR) $$(VFLAGS_BUILD) -Wno-SYNCASYNCNET \
	    $$(INTEG_EXTRA_FLAGS_$(1)) \
	    --top-module arm7tdmis_$(1)_tb \
	    --Mdir $$(OBJ_DIR)/integ_$(1) -f $$(SIM_F) \
	    $$(INTEG_COMMON_SRCS) \
	    ../tb/integration/arm7tdmis_$(1)_tb.sv
	./$$(OBJ_DIR)/integ_$(1)/Varm7tdmis_$(1)_tb \
	    $$(INTEG_RUN_ARGS_$(1))
endef

$(foreach t,$(INTEG_TESTS),$(eval $(call INTEG_RULE,$(t))))

integ: $(addprefix integ-, $(INTEG_TESTS))

errata: $(addprefix integ-, $(ERRATA_TESTS))

$(PUBLIC_SUITE_BIN): Makefile $(SIM_F) $(RTL_SRCS) \
                     ../tb/integration/arm7tdmis_public_suite_tb.sv
	mkdir -p $(PUBLIC_SUITE_OBJ_DIR)
	$(VERILATOR) $(VFLAGS_BUILD) -Wno-SYNCASYNCNET \
	    -DARM7TDMIS_VERIFICATION \
	    --top-module arm7tdmis_public_suite_tb \
	    --Mdir $(PUBLIC_SUITE_OBJ_DIR) -f $(SIM_F) \
	    ../tb/integration/arm7tdmis_public_suite_tb.sv

public-suite-build: $(PUBLIC_SUITE_BIN)

public-suite: public-suite-build
	PYTHONPATH=../verification $(PYTHON) ../verification/public_suite.py \
	    --binary $(abspath $(PUBLIC_SUITE_BIN)) \
	    --manifest ../verification/public_suites.json \
	    --output $(PUBLIC_SUITE_REPORT) $(PUBLIC_SUITE_ARGS)

# VAL-004 is deliberately a full-regression-only aggregate. It consumes the
# already completed directed phases and rejects missing, stale, unhashed, or
# marker-free Chapter 7 cross evidence.
table7-cross:
	PYTHONPATH=../verification $(PYTHON) ../verification/table7_cross.py \
	    --manifest verification/table7_cross.json \
	    --regression $(REGRESSION_REPORT) \
	    --output reports/generated/table7-cross-report.json

# VAL-006 consumes the exhaustive decoder/policy phases from the current
# clean full regression and closes the independently enumerated required bins.
functional-coverage:
	PYTHONPATH=../verification $(PYTHON) \
	    ../verification/functional_coverage.py \
	    --manifest verification/functional_coverage.json \
	    --regression $(REGRESSION_REPORT) \
	    --output reports/generated/functional-coverage-report.json

# VAL-007/VAL-008 use a release-pinned OSS CAD Suite archive and fail unless
# every required proof and every named bounded cover statement closes.
formal:
	$(PYTHON) install_oss_cad_suite.py
	PYTHONPATH=. $(PYTHON) formal_report.py \
	    --jobs $(FORMAL_JOBS) --output $(FORMAL_REPORT)

# Run a supplied image without building or invoking the QEMU scoreboards.
$(STANDALONE_BIN): Makefile $(SIM_F) $(RTL_SRCS) \
                   ../tb/integration/arm7tdmis_memory.sv \
                   ../tb/integration/arm7tdmis_standalone_tb.sv
	mkdir -p $(STANDALONE_DIR)
	$(VERILATOR) $(VFLAGS_COMMON) --binary -j $(STANDALONE_JOBS) \
	    -Wno-SYNCASYNCNET -DARM7TDMIS_VERIFICATION \
	    --top-module arm7tdmis_standalone_tb --Mdir $(STANDALONE_DIR) \
	    -f $(SIM_F) ../tb/integration/arm7tdmis_memory.sv \
	    ../tb/integration/arm7tdmis_standalone_tb.sv

.PHONY: standalone-build standalone-test
standalone-build: $(STANDALONE_BIN)

standalone-test: standalone-build
	ARM7_STANDALONE_BINARY=$(abspath $(STANDALONE_BIN)) \
	    $(PYTHON) -m unittest discover -s tests -p test_standalone_runner.py -v

# VAL-002 builds the scoreboards once, then generates fresh source/reference
# images and executes every deterministic seed through both oracle lanes.
$(RANDOM_DIFF_BIN): Makefile $(SIM_F) $(RTL_SRCS) \
                    ../tb/integration/arm7tdmis_memory.sv \
                    ../tb/integration/arm7tdmis_random_diff_tb.sv
	mkdir -p $(RANDOM_OBJ_DIR)/diff
	$(VERILATOR) $(VFLAGS_BUILD) -Wno-SYNCASYNCNET \
	    -DARM7TDMIS_VERIFICATION \
	    --top-module arm7tdmis_random_diff_tb \
	    --Mdir $(RANDOM_OBJ_DIR)/diff -f $(SIM_F) \
	    ../tb/integration/arm7tdmis_memory.sv \
	    ../tb/integration/arm7tdmis_random_diff_tb.sv

$(RANDOM_POLICY_BIN): Makefile $(SIM_F) $(RTL_SRCS) \
                      ../tb/integration/arm7tdmis_memory.sv \
                      ../tb/integration/arm7tdmis_random_policy_tb.sv
	mkdir -p $(RANDOM_OBJ_DIR)/policy
	$(VERILATOR) $(VFLAGS_BUILD) -Wno-SYNCASYNCNET \
	    --top-module arm7tdmis_random_policy_tb \
	    --Mdir $(RANDOM_OBJ_DIR)/policy -f $(SIM_F) \
	    ../tb/integration/arm7tdmis_memory.sv \
	    ../tb/integration/arm7tdmis_random_policy_tb.sv

random-validation-build: $(RANDOM_DIFF_BIN) $(RANDOM_POLICY_BIN)

random-validation: random-validation-build
	PYTHONPATH=../verification $(PYTHON) ../verification/constrained_random.py \
	    --differential-binary $(abspath $(RANDOM_DIFF_BIN)) \
	    --policy-binary $(abspath $(RANDOM_POLICY_BIN)) \
	    --output $(RANDOM_REPORT) \
	    --seeds $(RANDOM_SEEDS) --instructions $(RANDOM_INSTRUCTIONS) \
	    --clang $(CLANG) --objcopy $(LLVM_OBJCOPY) --nm $(LLVM_NM) \
	    --qemu $(QEMU_SYSTEM_ARM)

random-validation-quick: random-validation-build
	PYTHONPATH=../verification $(PYTHON) ../verification/constrained_random.py \
	    --differential-binary $(abspath $(RANDOM_DIFF_BIN)) \
	    --policy-binary $(abspath $(RANDOM_POLICY_BIN)) \
	    --output $(RANDOM_REPORT) \
	    --seeds 2 --instructions 64 \
	    --clang $(CLANG) --objcopy $(LLVM_OBJCOPY) --nm $(LLVM_NM) \
	    --qemu $(QEMU_SYSTEM_ARM)

# VAL-005 closes every normalized instruction-class CLKEN-stall bin and
# every legal external-event bin independently in each deterministic seed.
$(RANDOM_EVENT_BIN): Makefile $(SIM_F) $(RTL_SRCS) \
                     ../tb/integration/arm7tdmis_memory.sv \
                     ../tb/integration/arm7tdmis_random_event_matrix_tb.sv
	mkdir -p $(RANDOM_EVENT_OBJ_DIR)
	$(VERILATOR) $(VFLAGS_BUILD) -Wno-SYNCASYNCNET \
	    --top-module arm7tdmis_random_event_matrix_tb \
	    --Mdir $(RANDOM_EVENT_OBJ_DIR) -f $(SIM_F) \
	    ../tb/integration/arm7tdmis_memory.sv \
	    ../tb/integration/arm7tdmis_random_event_matrix_tb.sv

random-events-build: $(RANDOM_EVENT_BIN)

random-events: random-events-build
	PYTHONPATH=../verification $(PYTHON) ../verification/random_events.py \
	    --binary $(abspath $(RANDOM_EVENT_BIN)) \
	    --seeds $(RANDOM_EVENT_SEEDS) \
	    --output $(RANDOM_EVENT_REPORT)

# The release soak uses a separate, sanitizer-instrumented binary so ordinary
# developer simulations do not silently inherit host-specific sanitizer
# runtime requirements. Unique X initialization is controlled by Verilator's
# deterministic runtime seed, which is kept identical to the workload seed.
$(SOAK_BIN): Makefile $(SIM_F) $(RTL_SRCS) $(INTEG_COMMON_SRCS) \
             ../tb/integration/arm7tdmis_mister_wrapper_tb.sv
	mkdir -p $(SOAK_OBJ_DIR)
	$(VERILATOR) $(VFLAGS_COMMON) --binary -j 0 -Wno-SYNCASYNCNET \
	    --x-assign unique --x-initial unique \
	    -CFLAGS "-fsanitize=address,undefined -fno-omit-frame-pointer" \
	    -LDFLAGS "-fsanitize=address,undefined" \
	    --top-module arm7tdmis_mister_wrapper_tb \
	    --Mdir $(SOAK_OBJ_DIR) -f $(SIM_F) \
	    $(INTEG_COMMON_SRCS) \
	    ../tb/integration/arm7tdmis_mister_wrapper_tb.sv

soak-build: $(SOAK_BIN)

soak: soak-build
	PYTHONPATH=. $(PYTHON) soak_harness.py \
	    --binary $(abspath $(SOAK_BIN)) \
	    --seeds $(SOAK_SEEDS) \
	    --output $(SOAK_REPORT)

# Coverage builds are separate from ordinary trace builds and write one raw
# database per test. The top-level target merges those databases before
# producing LCOV and the versioned machine-readable summary.
define COVERAGE_RULE
coverage-run-$(1):
	mkdir -p $$(COVERAGE_OBJ_DIR)/$(1) $$(COVERAGE_REPORT_DIR)
	$$(VERILATOR) $$(VFLAGS_COVERAGE) -Wno-SYNCASYNCNET \
	    $$(INTEG_EXTRA_FLAGS_$(1)) \
	    --top-module arm7tdmis_$(1)_tb \
	    --Mdir $$(COVERAGE_OBJ_DIR)/$(1) -f $$(SIM_F) \
	    $$(INTEG_COMMON_SRCS) \
	    ../tb/integration/arm7tdmis_$(1)_tb.sv
	./$$(COVERAGE_OBJ_DIR)/$(1)/Varm7tdmis_$(1)_tb \
	    +verilator+coverage+file+$$(abspath $$(COVERAGE_REPORT_DIR))/$(1).dat
endef

$(foreach t,$(COVERAGE_TESTS),$(eval $(call COVERAGE_RULE,$(t))))

.PHONY: $(addprefix coverage-run-, $(COVERAGE_TESTS))

coverage:
	rm -rf $(COVERAGE_OBJ_DIR) $(COVERAGE_REPORT_DIR)
	mkdir -p $(COVERAGE_OBJ_DIR) $(COVERAGE_REPORT_DIR)
	@set -eu; \
	for test in $(COVERAGE_TESTS); do \
	    $(MAKE) coverage-run-$$test; \
	done
	$(VERILATOR_COVERAGE) --write $(COVERAGE_REPORT_DIR)/coverage.dat \
	    $(COVERAGE_RAW_FILES)
	$(VERILATOR_COVERAGE) --write-info $(COVERAGE_REPORT_DIR)/coverage.info \
	    $(COVERAGE_REPORT_DIR)/coverage.dat
	PYTHONPATH=. $(PYTHON) coverage_report.py \
	    --raw $(COVERAGE_REPORT_DIR)/coverage.dat \
	    --lcov $(COVERAGE_REPORT_DIR)/coverage.info \
	    --output $(COVERAGE_REPORT_DIR)/coverage.json \
	    --tests "$(COVERAGE_TESTS)"

traceability:
	PYTHONPATH=. $(PYTHON) traceability_report.py --check \
	    --output $(TRACEABILITY_REPORT)

trm-coverage:
	PYTHONPATH=. $(PYTHON) trm_coverage.py --check \
	    --output $(TRM_COVERAGE_REPORT)

ac-timing:
	PYTHONPATH=. $(PYTHON) ac_timing.py --check \
	    --output $(AC_TIMING_REPORT)

# Release-facing entry point. The runner always starts clean, records the
# complete source/tool/variant/seed manifest, executes every phase separately,
# and atomically updates a machine-readable JSON result with per-phase logs.
regress:
	PYTHONPATH=. $(PYTHON) regression_harness.py \
	    --unit-tests "$(UNIT_TESTS)" \
	    --integration-tests "$(INTEG_TESTS)"

regress-quick:
	PYTHONPATH=. $(PYTHON) regression_harness.py --quick \
	    --unit-tests "$(UNIT_TESTS)" \
	    --integration-tests "$(INTEG_TESTS)"

# Open-source CI uses the same report contract but cannot claim the Quartus
# phases. The report's scope and empty FPGA manifest make that distinction
# machine-readable.
regress-ci:
	PYTHONPATH=. $(PYTHON) regression_harness.py --quick --simulation-only \
	    --variant ci-open-source \
	    --unit-tests "$(UNIT_TESTS)" \
	    --integration-tests "$(INTEG_TESTS)"

release-evidence:
	PYTHONPATH=. $(PYTHON) release_evidence.py

harness-unit:
	PYTHONPATH=. $(PYTHON) -m unittest discover -s tests -p 'test_*.py' -v

harness-self-test:
	mkdir -p $(OBJ_DIR)
	$(VERILATOR) $(VFLAGS_BUILD) --top-module expected_failure_tb \
	    --Mdir $(OBJ_DIR)/harness_expected_failure \
	    ../tb/harness/expected_failure_tb.sv
	@if ./$(OBJ_DIR)/harness_expected_failure/Vexpected_failure_tb; then \
	    echo "[harness-self-test] FAIL: expected simulator failure returned zero"; \
	    exit 1; \
	else \
	    echo "[harness-self-test] PASS: simulator failure propagated nonzero"; \
	fi

raw-checker-self-test:
	mkdir -p $(OBJ_DIR)
	$(VERILATOR) $(VFLAGS_BUILD) --top-module raw_bus_checker_violation_tb \
	    --Mdir $(OBJ_DIR)/raw_checker_expected_failure \
	    -f $(SIM_F) \
	    ../verification/arm7tdmis_raw_bus_checker.sv \
	    ../tb/harness/raw_bus_checker_violation_tb.sv
	@set -eu; \
	bin=./$(OBJ_DIR)/raw_checker_expected_failure/Vraw_bus_checker_violation_tb; \
	"$$bin" +CASE=legal; \
	for spec in \
	    "reset_controls:reset must drive TRANS=I" \
	    "cfg_change:CFGBIGEND changed outside reset" \
	    "reserved_size:SIZE=2'b11 is reserved" \
	    "cpnmreq_mirror:CPnMREQ does not mirror TRANS active" \
	    "cpseq_mirror:CPSEQ does not mirror TRANS[0]" \
	    "cpnopc_mirror:CPnOPC does not mirror PROT[0]" \
	    "dmore_class:DMORE outside an unlocked word data address" \
	    "sequential_history:S address lacks legal burst history" \
	    "dmore_follower:DMORE promise lacked +4 S follower" \
	    "stall_mutation:output changed while CLKEN=0" \
	    "byte_burst:byte burst is forbidden"; do \
	    case_name=$${spec%%:*}; \
	    expected=$${spec#*:}; \
	    log=$(OBJ_DIR)/raw_checker_$${case_name}.log; \
	    if "$$bin" +CASE=$$case_name >"$$log" 2>&1; then \
	        echo "[raw-checker-self-test/$$case_name] FAIL: violation returned zero"; \
	        exit 1; \
	    fi; \
	    if ! grep -F "$$expected" "$$log" >/dev/null; then \
	        echo "[raw-checker-self-test/$$case_name] FAIL: wrong assertion"; \
	        cat "$$log"; \
	        exit 1; \
	    fi; \
	    echo "[raw-checker-self-test/$$case_name] PASS"; \
	done

mutation-self-test:
	PYTHONPATH=. $(PYTHON) mutation_harness.py

clean:
	rm -rf $(OBJ_DIR) $(RUN_ARTIFACTS)

help:
	@echo "Targets:"
	@echo "  lint         Verilator --lint-only on RTL only (default)"
	@echo "  lint-mister  Verilator --lint-only on the canonical FPGA wrapper"
	@echo "  lint-example Verilator --lint-only on the portable FPGA example"
	@echo "  lint-generic-soc Verilator lint on the ROM/RAM/timer/UART example"
	@echo "  lint-generic-soc-slang Slang compile of the generic SoC example"
	@echo "  generic-soc-rom-check prove ARM source equals the synthesized ROM"
	@echo "  generic-soc-rom-artifacts retain ELF/binary/disassembly under reports/"
	@echo "  lint-independent Slang zero-warning compile of all public tops"
	@echo "  cdc-rdc      structural single-clock/synchronizer/reset audit"
	@echo "  fpga-quality run independent lint and CDC/RDC gates"
	@echo "  sim-generic-soc Run the generic SoC architectural program"
	@echo "  quartus-analysis Quartus analysis/elaboration of the FPGA package"
	@echo "  quartus-compile Full Quartus synth/fit/timing/assembly/power flow"
	@echo "  quartus-conformance-analysis Quartus analysis of the raw/full profile"
	@echo "  quartus-conformance-compile Full Quartus/power raw/full profile"
	@echo "  quartus-option-characterization Fit/power all four option profiles"
	@echo "  postfit-sim  fit/simulate little- and big-endian wrapper netlists"
	@echo "  lint-tb      Verilator --lint-only including the integration TB"
	@echo "  sim          build the integration testbench binary"
	@echo "  run          build and run; emits cycle.csv + waves.fst"
	@echo "  wave         open waves.fst in GTKWave (requires \$$DISPLAY)"
	@echo "  unit-<name>  build + run a unit test (e.g. unit-regfile)"
	@echo "  unit         run every unit test ($(UNIT_TESTS))"
	@echo "  integ        run every directed integration test ($(INTEG_TESTS))"
	@echo "  errata       run the complete 15-entry r4p3 errata evidence matrix"
	@echo "  public-suite run pinned MIT ARM and Thumb instruction exercisers"
	@echo "  table7-cross validate all required Chapter 7 cross evidence in a full regression"
	@echo "  functional-coverage close all ARMv4T encoding/policy bins"
	@echo "  trm-coverage validate every r4p3 section, table, figure, and signal"
	@echo "  ac-timing validate every Chapter 8/Table 8-1 FPGA disposition"
	@echo "  mister-framework build the CPU in the pinned official MiSTer project"
	@echo "  formal       prove VAL-007 and reach every VAL-008 cover obligation"
	@echo "  standalone-build build the supplied-program runner (no QEMU)"
	@echo "  standalone-test verify standalone execution and JSONL capture"
	@echo "  random-validation run 32 x 256-instruction constrained-random seeds"
	@echo "  random-events run 32 all-class external-event seeds"
	@echo "  soak         run 256 deterministic sanitizer/X-propagation seeds"
	@echo "  regress      clean full run with JSON metadata/results under reports/"
	@echo "  regress-quick lint/harness plus one unit/integration test and smoke"
	@echo "  regress-ci   quick simulation-only CI report without Quartus phases"
	@echo "  coverage     measured Verilator coverage databases, LCOV, and JSON"
	@echo "  release-evidence validate and archive clean regression/coverage reports"
	@echo "  harness-unit unit-test the machine-readable regression harness"
	@echo "  harness-self-test prove an intentional simulator failure stays fatal"
	@echo "  mutation-self-test kill writeback/flag/exception/bus mutants"
	@echo "  clean        remove obj_dir + run artifacts"
