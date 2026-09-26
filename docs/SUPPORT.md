# Support matrix

The project version is recorded in [`VERSION`](../VERSION). Version
`0.9.0-dev` is a verification prerelease, not a signed drop-in CPU-IP
release. [`TASKS.md`](../TASKS.md) §31 is the authoritative readiness ledger.

| Surface or tool | Status | Checked scope |
|---|---|---|
| `arm7tdmi_mister` valid/ready API v1 | Supported for directed integration | RTL and two-endian fitted-netlist execution, randomized waits and CPU enables, errors, CDC/reset, all option profiles, DMA arbitration, and Quartus characterization |
| `arm7tdmis_top` raw API v1 | Supported for directed integration | ARM DDI 0234B pin semantics, reusable protocol checker, Chapter 7 directed phase matrices, and Quartus conformance profile |
| Synchronous debug-step bridge | Supported project-specific JTAG-006 transport | Same-CLK ready/valid virtual-TCK steps, backpressure, policy isolation, public-wrapper/QIP packaging, and real-TAP scan evidence; no asynchronous pod or GDB process is claimed |
| Verilator 5.x | Supported simulator and linter | Local release characterization used Verilator 5.048; CI installs its distribution version and records the exact result |
| Slang 11.0 | Supported independent SystemVerilog frontend | CI verifies the official Linux archive SHA-256; zero-warning elaboration covers all five public synthesis tops and records hashed diagnostics |
| QEMU system ARM | Supported independent shared-subset reference | The directed lane compares 77 retirements and the release random lane compares at least 8,192; ARM926 extensions, endian policy, and unaligned policy are not treated as ARM7 evidence |
| `jsmolka/gba-suite` at `a7113b67…` | Supported pinned public ARMv4T validation source | MIT license and source hashes are checked at runtime; ARM and Thumb images execute 15,287 retirements with a frozen signature after only manifest-listed header/policy normalization |
| Arm GNU Toolchain 14.3.Rel1 | Supported pinned compiler-program tool on Linux x86-64 | The archive SHA-256 and GCC identity are checked; separate ARM and Thumb C units execute bidirectional ARMv4T interworking and a fail-hard memory signature |
| Quartus Lite 17.0.2 | Supported characterization and post-fit tool | Analysis, synthesis, fit, assembly, and four-corner TimeQuest for Cyclone V `5CSEBA6U23I7`; temporary little-/big-endian functional netlists execute architectural wrapper scoreboards |
| GNU Make and Rust | Supported orchestration | The Rust runner records exact tool versions in regression and release manifests |
| Icarus Verilog 13.0 | Not supported | Its SystemVerilog frontend rejects package/type syntax used by this RTL |
| Pinned OSS CAD Suite Yosys/SymbiYosys/Boolector | Supported formal flow | The checksum-verified bundle closes 14/14 required proofs and 77/77 named reachability covers; this does not claim Yosys as a supported production FPGA synthesis flow |
| Official MiSTer template integration | Supported build-qualified target | Pinned upstream commit/tree, `sys_top`, 12.500 MHz CPU PLL clock, complete fitted constraints, positive four-corner timing, resources, and hashed RBF are checked; physical-board qualification remains FPGA-007 |
| PocketStation system or software images | Not included | System integration, legal user-supplied images, reference comparison, and soak evidence remain MIST-008/009 |

Only the two documented API version-1 surfaces are public compatibility
contracts. Internal module hierarchy, verification-only ports, and package
implementation details are not stable integration APIs.
