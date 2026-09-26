# Source and license provenance

This inventory applies to version `0.9.0-dev`. It records SPDX identifiers
where the license is known and uses `NOASSERTION` where this project cannot
make a broader licensing claim. It is not legal advice.

| Material | Origin and handling | SPDX license status |
|---|---|---|
| `rtl/`, `tb/`, `verification/`, `scripts/`, `fpga/`, project documentation, and checked text metadata | Authored for this repository; individual files do not currently carry SPDX headers | `GPL-3.0-only`, under the root `LICENSE` |
| `ARM_DDI_0234B_ARM7TDMI-S_r4p3_TRM.pdf` | Arm specification retained with its original notices; used as the behavioral reference and not relicensed by this project | `LicenseRef-Arm-DDI-0234B`; overall SPDX conclusion `NOASSERTION` |
| Published r4p3 errata notice | Not vendored; `ERRATA.md` records the official URL, size, SHA-256, review date, and derived dispositions | `NOASSERTION` |
| Test-program `.hex` files | Repository-authored hand encodings used only by the local verification benches | `GPL-3.0-only` |
| Generic-SoC FPGA smoke ROM | `examples/generic_soc/program.S` and `linker.ld` are repository-authored ARMv4T/Thumb-1 source. The checksum-installed Arm GNU 14.3.Rel1 toolchain produces only ignored inspection artifacts; a mandatory checker proves its 812-byte binary equals the synthesizable SystemVerilog ROM word for word | `GPL-3.0-only` |
| QEMU directed and constrained-random differential reference | No QEMU code is vendored. The installed `qemu-system-arm` executable is invoked as an independent tool; each generated metadata file records its exact version, command, and output hashes. ARM7-specific endian/unaligned expected values are repository-authored rather than copied from QEMU | Governed by QEMU's upstream license terms; generated register facts are not committed to the repository |
| Arm GNU Toolchain compiler-program evidence | No toolchain binary is vendored. The Rust `arm-toolchain-install` command downloads Arm GNU Toolchain 14.3.Rel1 from Arm's official release endpoint, verifies the recorded archive SHA-256, and installs it only in ignored local storage. Generated objects, ELF, disassembly, and signatures are not committed; release evidence records their tool/source/output metadata | Governed by the upstream Arm GNU Toolchain notices and component licenses |
| Intel Quartus fitted-netlist evidence | Quartus Lite 17.0.2 generates little- and big-endian Cyclone V functional netlists only in temporary storage. The netlists and Intel simulation libraries are not committed or redistributed; evidence retains hashes and repository-authored logs. `intel_cyclonev_postfit_primitives.sv` is a clean-room, zero-delay implementation of the five public primitive interfaces used by the validation profile and contains no copied Intel model source | Temporary Quartus outputs remain governed by the Intel tool license; the repository-authored primitive shim is `GPL-3.0-only` |
| `jsmolka/gba-suite` public ARM/Thumb CPU exercisers | No upstream source or ROM is committed. the Rust `public-suite` runner fetches the exact Git commit `a7113b67e63f83a9b321696ddd7042ccfad6c881` (tree `c561a03a85c3de8a39fa557b9aeced6dbffcfa3d`), verifies origin, clean checkout, source-ROM hashes, and LICENSE SHA-256 `b59a9ed235d76752c977f04116bc72155a14af48176bdbf50d7e5c29fca35aa7`, then creates ignored execution artifacts. Generated images zero the non-executable trademark/header bytes and alter only manifest-listed words that branch over documented out-of-policy cases; no generated ROM is redistributed from Git. The evidence archive includes the upstream license text, generated images, exact patch manifest, and hashes for reproducibility | `MIT` for the pinned upstream suite; repository-authored runner, manifest, and testbench are `GPL-3.0-only` |
| Firmware, BIOS, ROMs, games, and PocketStation software | None included. Users and downstream projects must supply only material they may lawfully use; it must not be committed to this repository | `NOASSERTION` |
| GitHub Actions | Workflow references `actions/checkout@v4` and `actions/upload-artifact@v4`; action source is fetched by GitHub and is not vendored here | Governed by each upstream action |

The release manifest records exact SHA-256 values for the included TRM and
root license, the Git tree and regression source digest, tool version and
executable manifests, and every archived evidence file. A new external
suite, emulator, firmware fragment, ROM, generated IP block, font, image, or
MiSTer component requires an entry here with origin, exact revision, license,
local modifications, and redistribution decision before it can become
release evidence.

Copyright authorship for repository-authored material follows Git history;
this document makes no additional ownership assignment. The root GPL license
does not alter the copyright or terms of the Arm manual or any future
user-supplied software.
