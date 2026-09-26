# ARM7TDMI-S implementation and release task ledger

## How to read this file

Sections 0–30 are the original build roadmap plus a later TRM addendum. They describe
work that a complete implementation requires; they are **not evidence that the work has
been completed**. In particular, an RTL block existing, a decoder recognizing an
instruction class, or a test printing `PASS` is not enough to mark a requirement done.

Section 31 is the canonical readiness audit and release gate. It was added after a
source-level and test-level audit of baseline commit
`23bb86a73f60f1b64d1929632befc532e736a148` on 2026-07-28. If a status statement
elsewhere in the repository conflicts with §31, §31 wins until the conflicting statement
is corrected and linked to passing evidence.

The 2026-07-29 revision audit re-read all 242 pages of ARM DDI 0234B r4p3
(`bb9cd0e3f2b7e2fdca4ff961cdfc5c9c85c063842e491f8997a504d3241baa14`) and
added the remaining manual-coverage obligations to §31.7, §31.11, and §31.12.
Those unchecked rows are real gaps even where a neighboring subsystem row is checked.

Use these status words consistently:

- **VERIFIED** — the requirement has a fail-hard, self-checking test against an
  identified authoritative requirement, and the release regression records it passing.
- **IMPLEMENTED, UNVERIFIED** — a plausible RTL path exists, but the required evidence
  does not.
- **PARTIAL** — only a subset or simplified/scaffold behavior exists.
- **INCORRECT** — the present RTL or test expectation conflicts with the selected
  specification.
- **MISSING** — no implementation or evidence was found.
- **OUT OF SCOPE** — excluded by an explicit release profile and replaced by a defined
  tie-off or adapter contract.

Nothing in this repository is currently signed off as a drop-in MiSTer CPU IP release.
The unchecked gates in §31 are the work needed to reach that claim.

---

# 0. Define the target

Your final target should be a synthesizable SystemVerilog implementation with these major blocks:

```text
arm7tdmis_top
├── clock/reset/clock-enable control
├── programmer-visible CPU core
│   ├── register bank: 31 x 32-bit GPRs + 6 status registers
│   ├── CPSR/SPSR logic
│   ├── ARM decoder
│   ├── Thumb decoder/decompressor
│   ├── barrel shifter
│   ├── ALU
│   ├── multiplier / multiply-accumulate unit
│   ├── address generator
│   ├── load/store unit
│   ├── exception/interrupt controller
│   └── 3-stage pipeline control
├── ARM7TDMI-S memory interface
├── coprocessor interface
├── internal CP14 debug communications channel
├── external CP15/system-control attachment or no-CP15 tie-off, as selected by profile
├── EmbeddedICE-RT debug macrocell
├── JTAG/TAP scan interface
├── ETM7-facing trace interface
└── synthesis/scan/timing wrapper
```

The TRM’s Figure 1-3 is the best architectural starting point: it shows the address register/incrementer, register bank, 32x8 multiplier, barrel shifter, 32-bit ALU, instruction pipeline, read/write data registers, Thumb decoder, instruction decoder/control logic, memory signals, debug signals, and coprocessor handshake signals. 

---

# 1. Build the project skeleton

Implement these first:

1. Create the repo structure:

```text
rtl/
  core/
  decode/
  datapath/
  memory/
  coproc/
  debug/
  jtag/
  etm/
  top/
tb/
  unit/
  integration/
  formal/
  programs/
docs/
scripts/
```

2. Define common SystemVerilog packages:

```text
arm7tdmis_types_pkg.sv
arm7tdmis_instr_pkg.sv
arm7tdmis_psr_pkg.sv
arm7tdmis_bus_pkg.sv
arm7tdmis_debug_pkg.sv
```

3. Define enums for:

```text
ARM/Thumb state
processor modes
exception types
memory transfer size
memory transfer type
ALU operation
shifter operation
pipeline stage state
coprocessor handshake state
debug state
```

4. Define your top-level pin list directly from the TRM:

```text
CLK
CLKEN
nRESET
CFGBIGEND
nIRQ
nFIQ
ABORT

ADDR[31:0]
WRITE
SIZE[1:0]
PROT[1:0]
LOCK
TRANS[1:0]
WDATA[31:0]
RDATA[31:0]

CPnMREQ
CPSEQ
CPnTRANS
CPnOPC
CPTBIT
CPnI
CPA
CPB

DBGEN
DBGRQ
DBGBREAK
DBGACK
DBGnEXEC
DBGINSTRVALID
DBGEXT[1:0]
DBGRNG[1:0]
DBGCOMMTX
DBGCOMMRX

DBGTCKEN
DBGTMS
DBGTDI
DBGTDO
DBGnTRST
DBGnTDOEN
DMORE
```

The memory bus is grouped into clock/control, address-class, memory request, and data-timed signals; the TRM lists `CLK`, `CLKEN`, `nRESET`, `ADDR`, `WRITE`, `SIZE`, `PROT`, `LOCK`, `TRANS`, `WDATA`, `RDATA`, and `ABORT` as the main memory-interface signals. 

---

# 2. Build the verification framework before the CPU

Before writing the full core, build the test environment.

Tasks:

1. Create a simple behavioral memory model.

2. Support byte, halfword, and word accesses.

3. Support little-endian and big-endian modes using `CFGBIGEND`.

4. Support `ABORT` injection.

5. Support wait states using `CLKEN`.

6. Create a simple program loader for assembled ARM/Thumb binaries.

7. Create a cycle logger that records:

```text
cycle
PC
CPSR
mode
ARM/Thumb state
fetched instruction
decoded instruction
executed instruction
ADDR
WRITE
SIZE
PROT
LOCK
TRANS
WDATA
RDATA
exception state
debug state
```

8. Create assertion checks for:

```text
no register write when instruction condition fails
correct PC value visible to instructions
correct CPSR flag updates
correct exception vectoring
correct memory transaction type
correct byte-lane extraction
correct pipeline flush on branch/exception
```

9. Build directed tests first, then random instruction tests later.

10. Build a reference model path. This can be a small non-synthesizable interpreter in Python/C++/SystemVerilog testbench code, or comparison against another trusted ARMv4T emulator.

---

# 3. Implement the programmer-visible state

Implement the architectural state before the pipeline.

Tasks:

1. Implement the full register bank.

2. Implement User/System registers:

```text
r0-r15
CPSR
```

3. Implement banked FIQ registers:

```text
r8_fiq-r14_fiq
SPSR_fiq
```

4. Implement banked IRQ registers:

```text
r13_irq
r14_irq
SPSR_irq
```

5. Implement banked Supervisor registers:

```text
r13_svc
r14_svc
SPSR_svc
```

6. Implement banked Abort registers:

```text
r13_abt
r14_abt
SPSR_abt
```

7. Implement banked Undefined registers:

```text
r13_und
r14_und
SPSR_und
```

8. Implement CPSR fields:

```text
N Z C V
I
F
T
M[4:0]
reserved bits
```

9. Implement mode decode:

```text
User       10000
FIQ        10001
IRQ        10010
Supervisor 10011
Abort      10111
Undefined  11011
System     11111
```

10. Implement register-read mapping for every mode.

11. Implement register-write mapping for every mode.

12. Implement SPSR access rules.

13. Implement PC read behavior for ARM state.

14. Implement PC read behavior for Thumb state.

15. Implement PC write behavior and pipeline flush request.

The ARM7TDMI-S has 37 total registers: 31 general-purpose 32-bit registers and 6 status registers. Which registers are visible depends on current processor mode and ARM/Thumb state. 

---

# 4. Implement reset and basic mode control

Tasks:

1. Implement asynchronous or externally synchronized `nRESET` behavior according to your top-level choice.

2. On reset:

```text
mode = Supervisor
I = 1
F = 1
T = 0
PC = 0x00000000
ARM state
```

3. Treat other registers as architecturally indeterminate after reset.

4. Flush pipeline on reset.

5. Start fetching from vector address `0x00000000`.

The TRM states that when `nRESET` is released, the core enters Supervisor mode, sets the I and F bits, clears T, forces the PC to fetch from address `0x00`, and resumes in ARM state. 

---

# 5. Implement the core datapath primitives

Build these modules independently and unit-test each one.

## 5.1 Barrel shifter

Implement:

```text
LSL immediate
LSL register
LSR immediate
LSR register
ASR immediate
ASR register
ROR immediate
ROR register
RRX
```

Tasks:

1. Produce shifted result.

2. Produce shifter carry-out.

3. Handle special shift amounts.

4. Handle immediate rotate encoding for ARM data-processing immediates.

5. Support register-controlled shifts using the bottom 8 bits of the shift register.

## 5.2 ALU

Implement:

```text
AND
EOR
SUB
RSB
ADD
ADC
SBC
RSC
TST
TEQ
CMP
CMN
ORR
MOV
BIC
MVN
```

Tasks:

1. Generate result.

2. Generate N/Z flags.

3. Generate carry/borrow flag.

4. Generate signed overflow flag.

5. Support flag write enable.

6. Support compare/test instructions that update flags but do not write a destination register.

## 5.3 Multiplier

Implement:

```text
MUL
MLA
UMULL
UMLAL
SMULL
SMLAL
```

Tasks:

1. Implement 32x32 multiply.

2. Implement signed and unsigned long results.

3. Implement accumulate variants.

4. Implement flag updates for `S` variants.

5. Add cycle-control hooks for ARM7TDMI-S multiply timing.

The TRM’s core diagram includes a `32 x 8` multiplier, and Chapter 7 states that multiply instructions use special hardware with early termination.  

---

# 6. Implement condition-code evaluation

Every ARM instruction is conditional.

Tasks:

1. Decode `cond[31:28]`.

2. Implement:

```text
EQ NE CS CC MI PL VS VC HI LS GE LT GT LE AL
```

3. Generate `condition_pass`.

4. Suppress register writes when the condition fails.

5. Suppress memory writes when the condition fails.

6. Suppress CPSR writes when the condition fails.

7. Generate `DBGnEXEC` correctly for instructions that reach execute but fail the condition.

8. Confirm that unexecuted instructions still consume correct cycles.

The TRM notes that ARM-state instructions can execute conditionally, while in Thumb state only branch instructions are conditional. 

---

# 7. Implement a simple non-pipelined ARM execution model

This is a temporary milestone, not the final core.

Tasks:

1. Fetch one 32-bit instruction.

2. Decode it.

3. Execute it.

4. Update architectural state.

5. Ignore exact cycle timing for now.

6. Run small assembly programs.

7. Verify ALU, flags, branches, loads, stores, and exceptions at an architectural level.

This gives you a known-good core before adding pipeline timing.

---

# 8. Implement ARM instruction decode

Build a clean decoder that produces a normalized internal micro-operation.

## ARM decode groups

Implement decode for:

1. Branch:

```text
B
BL
BX
```

2. Data processing:

```text
AND EOR SUB RSB ADD ADC SBC RSC
TST TEQ CMP CMN
ORR MOV BIC MVN
```

3. PSR transfer:

```text
MRS CPSR
MRS SPSR
MSR CPSR
MSR SPSR
```

4. Multiply:

```text
MUL
MLA
UMULL
UMLAL
SMULL
SMLAL
```

5. Single data transfer:

```text
LDR
STR
LDRB
STRB
LDRT
STRT
LDRBT
STRBT
```

6. Halfword and signed transfer:

```text
LDRH
STRH
LDRSB
LDRSH
```

7. Block transfer:

```text
LDM
STM
all IA/IB/DA/DB addressing forms
writeback
user-register transfer
PC-in-list behavior
S-bit behavior
```

8. Swap:

```text
SWP
SWPB
```

9. Coprocessor instructions:

```text
CDP
MCR
MRC
LDC
STC
```

10. Software interrupt:

```text
SWI
```

11. Undefined instructions.

The TRM summarizes the ARM instruction set groups as move/PSR transfer, arithmetic, multiply, compare/test, logical, branch, load/store, block transfer, swap, coprocessor, and software interrupt instructions. 

---

# 9. Implement ARM data-processing execution

Tasks:

1. Decode operand 2:

```text
immediate rotated right
register
register shifted by immediate
register shifted by register
RRX
```

2. Read `Rn`.

3. Read `Rm`.

4. Read `Rs` when needed.

5. Run barrel shifter.

6. Run ALU.

7. Write `Rd` when applicable.

8. Update flags when `S = 1`.

9. Handle `Rd = PC`.

10. Handle `S = 1` and `Rd = PC` restoring CPSR from SPSR where architecturally required.

11. Handle `PC` as operand.

12. Verify every opcode with directed tests.

Chapter 7 says data operations normally execute in one datapath cycle, except register-specified shifts, and that writing the PC invalidates the current pipeline and forces a pipeline refill. 

---

# 10. Implement ARM branch instructions

Tasks:

1. Implement `B`.

2. Implement `BL`.

3. Implement `BX`.

4. Implement sign-extension of branch offset.

5. Implement link register update.

6. Implement ARM-to-Thumb transition through `BX`.

7. Implement Thumb-to-ARM transition through `BX`.

8. Flush/refill pipeline after branch.

9. Verify PC and LR values.

10. Verify branch timing later against Chapter 7.

The TRM states that ARM/Thumb branches and ARM `BL` take three cycles, and that `BX` takes three cycles while extracting both the destination address and new core state from the register source.  

---

# 11. Implement ARM load/store address generation

Tasks:

1. Implement Addressing Mode 2:

```text
immediate offset
register offset
scaled register offset
pre-indexed
post-indexed
up/down
writeback
byte/word transfer
T/user-mode transfer variants
```

2. Implement Addressing Mode 3:

```text
halfword
signed halfword
signed byte
immediate offset
register offset
pre-indexed
post-indexed
writeback
```

3. Implement effective address generation.

4. Implement base register writeback.

5. Implement PC-relative loads.

6. Implement byte extraction.

7. Implement halfword extraction.

8. Implement sign extension for `LDRSB` and `LDRSH`.

9. Implement unaligned behavior according to ARMv4T rules.

10. Implement data abort suppression rules.

11. Implement destination `PC` load pipeline refill.

The memory system must support byte, halfword, and word quantities; words are aligned to 4-byte boundaries, halfwords to 2-byte boundaries, and byte accesses can occur on any byte boundary. 

---

# 12. Implement ARM block data transfer

Tasks:

1. Implement `LDM`.

2. Implement `STM`.

3. Support all addressing modes:

```text
IA
IB
DA
DB
FD
ED
FA
EA aliases at assembly level, if you build assembler-facing tests
```

4. Implement register-list iteration order.

5. Implement base writeback.

6. Implement empty register-list behavior according to architecture rules.

7. Implement `^` user-register transfer behavior.

8. Implement `LDM ... PC` behavior.

9. Implement `LDM ... PC^` CPSR restore behavior.

10. Implement abort behavior.

11. Implement `DMORE` for Rev 4 multiple transfers.

12. Verify all register-list combinations.

13. Verify cycle timing for 1-register and n-register transfers.

Chapter 7 describes `LDM` as calculating the first address, fetching words sequentially, latching the modified base for abort recovery, preventing register writes after abort, and invalidating the pipeline if PC is loaded. 

---

# 13. Implement ARM swap

Tasks:

1. Implement `SWP`.

2. Implement `SWPB`.

3. Generate locked bus access using `LOCK`.

4. Perform read cycle.

5. Perform write cycle.

6. Update destination register only after successful read/write sequence.

7. Handle aborts.

8. Verify atomic read/write bus behavior.

The TRM states that `LOCK` is asserted for `SWP`/`SWPB`, and Chapter 7 shows `LOCK` high for both load and store data cycles of the swap operation.  

---

# 14. Implement exceptions and interrupts

Implement exceptions only after ARM instruction execution mostly works.

Tasks:

1. Implement exception types:

```text
Reset
Undefined instruction
Software interrupt
Prefetch abort
Data abort
IRQ
FIQ
```

2. Implement exception vectors:

```text
0x00000000 Reset
0x00000004 Undefined
0x00000008 SWI
0x0000000C Prefetch Abort
0x00000010 Data Abort
0x00000014 Reserved
0x00000018 IRQ
0x0000001C FIQ
```

3. Implement exception priority:

```text
Reset
Data Abort
FIQ
IRQ
Prefetch Abort
Undefined instruction
SWI
```

4. On exception entry:

```text
save return address into mode-specific LR
copy CPSR to mode-specific SPSR
change mode bits
set interrupt disable bits as required
clear T bit
force ARM state
load PC with vector address
flush/refill pipeline
```

5. Implement exception return behavior.

6. Implement `MOVS PC, LR` and `SUBS PC, LR, #imm` return paths.

7. Implement `FIQ` banked registers.

8. Implement `IRQ` masking through CPSR.I.

9. Implement `FIQ` masking through CPSR.F.

10. Implement `nIRQ` and `nFIQ` sampling.

11. Implement data abort behavior for:

```text
single load/store
swap
LDM/STM
```

12. Implement prefetch abort tracking through the pipeline.

13. Verify exception entry from ARM state.

14. Verify exception entry from Thumb state.

15. Verify exception return to ARM state.

16. Verify exception return to Thumb state.

The TRM gives the exception entry/exit behavior, including saving return addresses in the appropriate LR, copying CPSR to SPSR, changing mode, forcing vector fetch, and handling exceptions in ARM state even when the exception occurred in Thumb state. 

---

# 15. Implement Thumb instruction support

Thumb is not just a smaller decoder; it changes fetch width, PC behavior, available registers, and branch behavior.

Tasks:

1. Implement Thumb fetch alignment.

2. Implement Thumb PC behavior.

3. Implement Thumb decoder.

4. Decode Thumb instructions into the same internal micro-op format used by ARM where possible.

5. Implement Thumb low-register ALU operations:

```text
MOV
ADD
SUB
ADC
SBC
NEG
CMP
CMN
MUL
AND
EOR
ORR
BIC
MVN
TST
```

6. Implement Thumb shifts:

```text
LSL
LSR
ASR
ROR
```

7. Implement high-register operations:

```text
MOV high/low
ADD high/low
CMP high/low
BX
```

8. Implement immediate operations:

```text
MOV immediate
CMP immediate
ADD immediate
SUB immediate
```

9. Implement PC-relative load.

10. Implement SP-relative load/store.

11. Implement register-offset load/store:

```text
LDR
STR
LDRB
STRB
LDRH
STRH
LDRSB
LDRSH
```

12. Implement immediate-offset load/store.

13. Implement address-generation pseudo-ops:

```text
ADD Rd, PC, #imm
ADD Rd, SP, #imm
ADD/SUB SP, #imm
```

14. Implement multiple transfer:

```text
LDMIA
STMIA
PUSH
POP
PUSH {..., LR}
POP {..., PC}
```

15. Implement conditional branches.

16. Implement unconditional branch.

17. Implement Thumb long branch with link.

18. Implement Thumb `SWI`.

19. Implement Thumb-to-ARM exception entry.

20. Verify every Thumb encoding group.

The TRM states that ARM7TDMI-S has two states: ARM state executes 32-bit word-aligned ARM instructions, and Thumb state executes 16-bit halfword-aligned Thumb instructions. It also states that exceptions are handled in ARM state and that Thumb exceptions automatically switch to ARM state. 

---

# 16. Replace the simple execution model with the 3-stage pipeline

Now make the implementation structurally match ARM7TDMI-S.

Tasks:

1. Implement pipeline stages:

```text
Fetch
Decode
Execute
```

2. Implement instruction fetch stage.

3. Implement decode stage.

4. Implement execute stage.

5. Implement pipeline valid bits.

6. Implement pipeline flush on:

```text
branch
PC write
exception
debug entry
reset
state switch
```

7. Implement pipeline refill.

8. Implement ARM PC offset behavior:

```text
execute-stage instruction sees PC ahead due to pipeline
```

9. Implement Thumb PC offset behavior.

10. Implement interlocks for multi-cycle instructions.

11. Implement multi-cycle sequencing for:

```text
register-controlled shifts
multiply
load/store
LDM/STM
SWP
coprocessor transfers
exceptions
debug state
```

12. Implement instruction prefetch during execute when allowed.

13. Implement internal cycles when no useful memory access is possible.

14. Verify that branches, exceptions, and loads to PC refill the pipeline correctly.

The ARM7TDMI-S uses a three-stage Fetch/Decode/Execute pipeline, and the PC points to the instruction being fetched rather than the instruction being executed. 

---

# 17. Implement the ARM7TDMI-S memory interface

This is where your core becomes ARM7TDMI-S-like rather than just ARMv4T-compatible.

Tasks:

1. Implement the pipelined bus interface.

2. Generate address-class signals one bus cycle ahead of data transfer:

```text
ADDR[31:0]
WRITE
SIZE[1:0]
PROT[1:0]
LOCK
```

3. Generate transaction type:

```text
TRANS[1:0]
00 I-cycle
01 C-cycle
10 N-cycle
11 S-cycle
```

4. Implement N-cycle generation.

5. Implement S-cycle generation.

6. Implement I-cycle generation.

7. Implement C-cycle generation.

8. Implement merged I-S cycle behavior.

9. Implement sequential bursts:

```text
word read burst
word write burst
halfword read burst
```

10. Implement `CLKEN` bus-cycle extension.

11. Hold/update internal bus state only when `CLKEN` allows the bus cycle to complete.

12. Implement `RDATA` sampling.

13. Implement `WDATA` drive timing.

14. Implement `ABORT` sampling.

15. Implement `SIZE` encoding:

```text
00 byte
01 halfword
10 word
11 reserved
```

16. Implement `PROT` encoding:

```text
user/privileged
opcode/data
```

17. Implement `LOCK`.

18. Implement `DMORE` for LDM/STM sequential data access indication.

19. Implement little-endian byte/halfword extraction.

20. Implement big-endian byte/halfword extraction.

21. Implement byte/halfword write-data replication.

The TRM states that the bus interface is pipelined, with address-class and memory-request signals broadcast one bus cycle ahead of the bus cycle they refer to. It also defines the four `TRANS[1:0]` cycle types: internal, coprocessor register transfer, nonsequential, and sequential. 

---

# 18. Make the core cycle-accurate

Once architectural execution works, match the timing tables.

Tasks:

1. Create a cycle-count test suite from Chapter 7.

2. Verify normal data-processing timing.

3. Verify register-controlled shift timing.

4. Verify data-processing with destination PC timing.

5. Verify branch timing.

6. Verify ARM `BL` timing.

7. Verify Thumb `BL` timing.

8. Verify `BX` timing.

9. Verify multiply timing.

10. Verify multiply-accumulate timing.

11. Verify long multiply timing.

12. Verify load timing.

13. Verify store timing.

14. Verify `LDM` timing.

15. Verify `STM` timing.

16. Verify `SWP` timing.

17. Verify `SWI` and exception-entry timing.

18. Verify coprocessor timing.

19. Verify undefined instruction timing.

20. Verify unexecuted instruction timing.

21. Verify bus `TRANS`, `PROT`, `SIZE`, `WRITE`, and `LOCK` values cycle by cycle.

The TRM’s Chapter 7 explicitly gives instruction cycle timings for branches, data operations, multiply, load/store, block transfer, swap, SWI/exception entry, coprocessor operations, undefined instructions, and unexecuted instructions. 

---

# 19. Implement coprocessor instruction handling

Tasks:

1. Decode ARM coprocessor instructions:

```text
CDP
MRC
MCR
LDC
STC
```

2. Generate coprocessor pipeline-following signals:

```text
CPnMREQ
CPSEQ
CPnTRANS
CPnOPC
CPTBIT
```

3. Generate `CPnI`.

4. Sample `CPA`.

5. Sample `CPB`.

6. Implement coprocessor absent behavior.

7. Implement coprocessor busy-wait behavior.

8. Implement coprocessor ready behavior.

9. Implement abandonment of busy-waiting coprocessor instruction on interrupt/debug request.

10. Implement C-cycle bus usage.

11. Implement data transfer from ARM register to coprocessor for `MCR`.

12. Implement data transfer from coprocessor to ARM register for `MRC`.

13. Implement memory-to-coprocessor transfer for `LDC`.

14. Implement coprocessor-to-memory transfer for `STC`.

15. Implement undefined instruction trap when no coprocessor accepts the instruction.

16. Implement privileged coprocessor behavior using `CPnTRANS`.

17. Tie off external coprocessor inputs correctly when no external coprocessor is present.

The TRM says each coprocessor tracks the ARM pipeline, uses `CPnI`, `CPA`, and `CPB` for handshaking, and that unaccepted coprocessor instructions take the undefined instruction trap. 

---

# 20. Implement internal coprocessor 14: Debug Communications Channel

Tasks:

1. Implement CP14 decode for DCC instructions.

2. Implement DCC control register.

3. Implement DCC data write register.

4. Implement DCC data read register.

5. Implement processor-to-debugger transfer.

6. Implement debugger-to-processor transfer.

7. Implement `DBGCOMMTX`.

8. Implement `DBGCOMMRX`.

9. Implement JTAG access to DCC through scan chain 2.

10. Implement DCC status bits.

11. Verify polling sequences from processor side.

12. Verify polling sequences from debugger side.

The TRM states that the EmbeddedICE-RT DCC is implemented as coprocessor 14, has a 32-bit control register and 32-bit data register, and is accessed by the processor using `MCR`/`MRC` instructions to CP14. 

---

# 21. Handle CP15/system-control attachment correctly

Tasks:

1. Treat CP15 as an external coprocessor allocation, not an internal register bank in
   the bare ARM7TDMI-S profile.

2. With no system coprocessor attached, use `CPA=CPB=1` behavior and take Undefined
   for executed CP15 instructions.

3. If a SoC profile needs an MMU/MPU/system coprocessor, implement it outside the CPU
   and make it claim p15 through the normal coprocessor protocol.

4. Do not fabricate an ARM7TDMI-S Main ID register. ARM DAI 0099C explicitly states
   that ARM7TDMI and ARM7TDMI-S do not have CP15.

5. Verify absent-p15 Undefined behavior, condition-failed suppression, User/privileged
   `CPnTRANS`, and an attached external p15 independently.

The encoding space reserves coprocessor 15 for a system control coprocessor, but that
does not mean the bare ARM7TDMI-S macrocell contains one. External user coprocessors
must still use CP4–CP7.

---

# 22. Implement EmbeddedICE-RT debug macrocell

This is a large subsystem. Do it after the CPU core is stable.

Tasks:

1. Implement `DBGEN`.

2. Implement permanent debug disable behavior when `DBGEN` is low.

3. Implement debug request input `DBGRQ`.

4. Implement breakpoint/watchpoint input `DBGBREAK`.

5. Implement debug acknowledge `DBGACK`.

6. Implement debug state machine.

7. Implement halt-mode debugging.

8. Implement monitor-mode debugging.

9. Implement debug entry on breakpoint.

10. Implement debug entry on watchpoint.

11. Implement debug entry on debug request.

12. Implement debug exit sequence.

13. Implement debug interaction with exceptions.

14. Implement debug PC behavior.

15. Implement interrupt masking during debug state.

16. Implement system-speed access during debug.

17. Implement internal-cycle forcing while in debug state.

18. Implement two watchpoint units.

19. For each watchpoint unit, implement:

```text
address value register
address mask register
data value register
data mask register
control value register
control mask register
```

20. Implement breakpoint mode using instruction fetch matching.

21. Implement watchpoint mode using data access matching.

22. Implement data-dependent breakpoints/watchpoints.

23. Implement range matching.

24. Implement chained watchpoints.

25. Implement `DBGRNG[1:0]`.

26. Implement `DBGEXT[1:0]`.

27. Implement debug abort status register.

28. Implement debug control register.

29. Implement debug status register.

30. Implement EmbeddedICE-RT disable bit.

31. Implement programming sequence for breakpoint/watchpoint registers.

32. Verify breakpoint behavior with conditional instructions.

33. Verify watchpoint behavior with LDM/STM.

34. Verify watchpoint behavior with abort.

35. Verify monitor-mode abort exception entry.

36. Verify halt-mode debug entry.

The TRM describes EmbeddedICE-RT as including two real-time watchpoint units, an abort status register, the DCC, and debug control/status registers. The watchpoint units can be configured as watchpoints or breakpoints and can mask address/data/control bits. 

---

# 23. Implement JTAG TAP and scan chains

Tasks:

1. Implement TAP controller state machine.

2. Implement `DBGnTRST`.

3. Implement `DBGTCKEN`.

4. Implement `DBGTMS`.

5. Implement `DBGTDI`.

6. Implement `DBGTDO`.

7. Implement `DBGnTDOEN`.

8. Implement public JTAG instructions.

9. Implement instruction register.

10. Implement IDCODE register.

11. Implement TAP ID value `0x7F1F0F0F`.

12. Implement scan chain 1.

13. Implement scan chain 2.

14. Implement scan chain selection.

15. Implement DCC access through scan chain 2.

16. Implement EmbeddedICE-RT register access through scan chain 2.

17. Implement scan timing behavior.

18. Verify TAP reset.

19. Verify every TAP state transition.

20. Verify shifting/capturing/updating data registers.

The TRM states that ARM7TDMI-S has JTAG-style scan chains controlled by a TAP controller, and that Rev 4 has TAP ID register value `0x7F1F0F0F`.  

---

# 24. Implement ETM7-facing interface

You do not need to implement the whole ETM7 trace macrocell unless that is part of your project. But a complete ARM7TDMI-S-compatible macrocell should provide the ETM-facing signals.

Tasks:

1. Implement `DBGINSTRVALID`.

2. Implement `DBGnEXEC`.

3. Implement `CPTBIT`.

4. Expose `ADDR`.

5. Expose `RDATA`.

6. Expose `WDATA`.

7. Expose `ABORT`.

8. Expose `CPA`.

9. Expose `CPB`.

10. Expose `CPnMREQ`.

11. Expose `CPSEQ`.

12. Expose `CPnI`.

13. Expose `CPnOPC`.

14. Expose `SIZE`.

15. Expose `WRITE`.

16. Expose `PROT`.

17. Wire debug request compatibility for ETM.

18. Tie unsupported ETM processor-ID inputs low if you build an ETM wrapper.

19. Verify trace-visible instruction valid behavior.

20. Verify trace behavior across branches, exceptions, debug entry, and Thumb/ARM state changes.

The TRM’s ETM chapter says an external ETM can be connected for real-time tracing, generally with little or no glue logic, and gives the ETM7-to-ARM7TDMI-S signal mapping.  

---

# 25. Implement scan/test wrapper behavior

Tasks:

1. Implement normal-operation scan-test controls if your target includes ATPG-facing pins.

2. Implement:

```text
SCANENABLE
SCANIN
SCANOUT
```

3. Keep scan logic outside the core functional path where possible.

4. Make scan wrapper optional through parameters or synthesis defines.

5. Verify normal mode when `SCANENABLE = 0`.

6. Verify scan shift mode if you are targeting ASIC-style test insertion.

Appendix B describes an ATPG scan interface with `SCANENABLE`, `SCANIN`, and `SCANOUT`. 

---

# 26. Implement AC/timing-conscious RTL structure

Tasks:

1. Ensure all architectural state updates occur on `posedge CLK`.

2. Ensure `CLKEN` gates core/bus progress correctly.

3. Ensure debug clock-enable behavior uses `DBGTCKEN`.

4. Keep asynchronous reset handling consistent across modules.

5. Register long decode paths where necessary.

6. Avoid combinational loops in pipeline control.

7. Avoid inferred latches.

8. Split large decoders into staged decode where timing requires it.

9. Add synthesis constraints for clock, reset, and I/O timing.

10. Run lint.

11. Run CDC/reset checks if your debug clocking wrapper has asynchronous inputs.

12. Run synthesis after every major milestone.

13. Track maximum frequency, area, and critical path.

The TRM notes that all inputs are sampled on the rising edge of `CLK`, with clock enables sampled on every rising edge and other inputs sampled when the relevant clock enable is active. 

---

# 27. Build the conformance test suite

You need several layers of tests.

## Unit tests

1. Register bank mode mapping.

2. CPSR/SPSR writes.

3. ALU flags.

4. Barrel shifter carry-out.

5. Multiply results.

6. Decode tables.

7. Address generation.

8. Byte/halfword extraction.

9. Endianness.

10. Exception priority encoder.

11. TAP controller state transitions.

12. Watchpoint comparator.

## Instruction tests

1. Every ARM data-processing opcode.

2. Every ARM shifter mode.

3. Every ARM condition code.

4. Every branch type.

5. Every load/store addressing mode.

6. Every block transfer addressing mode.

7. Every multiply instruction.

8. Every swap instruction.

9. Every PSR instruction.

10. Every SWI/undefined path.

11. Every Thumb instruction group.

12. ARM/Thumb interworking.

## System tests

1. Reset boot from address `0x00000000`.

2. IRQ handling.

3. FIQ handling.

4. Nested exceptions.

5. Data abort retry.

6. Prefetch abort retry.

7. Undefined instruction trap.

8. SWI handler.

9. Thumb exception and return.

10. Debug halt entry/exit.

11. Monitor-mode debug.

12. JTAG DCC communication.

13. Coprocessor absent trap.

14. Coprocessor busy-wait and interrupt abandonment.

15. ETM-visible trace signal sanity.

## Cycle tests

1. Compare every instruction class against Chapter 7.

2. Compare bus transactions cycle by cycle.

3. Test with `CLKEN` always high.

4. Test with random `CLKEN` stalls.

5. Test with random memory aborts.

6. Test with random IRQ/FIQ arrival.

7. Test with random debug requests.

---

# 28. Recommended build order

This is the practical sequence I would follow.

## Milestone 1 — Minimal ARM core

1. Register bank.

2. CPSR.

3. ALU.

4. Barrel shifter.

5. ARM data-processing instructions.

6. Simple fetch/decode/execute loop.

7. Simple memory model.

8. Basic `B`, `BL`.

9. Basic `LDR`, `STR`.

10. Run simple ARM assembly programs.

## Milestone 2 — Full ARM architectural correctness

1. All ARM data-processing forms.

2. All ARM condition codes.

3. PSR transfers.

4. Multiply instructions.

5. All single data transfers.

6. Halfword/signed transfers.

7. Block transfers.

8. Swap.

9. SWI.

10. Undefined instruction.

11. Exceptions.

12. IRQ/FIQ.

13. Abort handling.

## Milestone 3 — Thumb support

1. Thumb decoder.

2. Thumb ALU instructions.

3. Thumb load/store instructions.

4. Thumb branches.

5. Thumb `BL`.

6. Thumb `BX`.

7. Thumb stack operations.

8. Thumb SWI.

9. ARM/Thumb interworking.

10. Thumb exception return.

## Milestone 4 — Real ARM7TDMI-S pipeline

1. Fetch/decode/execute pipeline.

2. PC-visible behavior.

3. Pipeline flush/refill.

4. Multi-cycle execute control.

5. Instruction prefetch behavior.

6. Cycle timing for branches/data operations.

7. Cycle timing for loads/stores.

8. Cycle timing for LDM/STM/SWP.

## Milestone 5 — ARM7TDMI-S bus accuracy

1. Full memory interface.

2. `TRANS` cycle generation.

3. `PROT`, `SIZE`, `WRITE`, `LOCK`.

4. Pipelined address/data timing.

5. `CLKEN` stalls.

6. Endianness.

7. Abort sampling.

8. Merged I-S cycles.

9. `DMORE`.

## Milestone 6 — Coprocessor interface

1. Coprocessor decode.

2. `CPnI`, `CPA`, `CPB`.

3. Busy-wait.

4. Absent coprocessor trap.

5. `MRC`, `MCR`.

6. `LDC`, `STC`.

7. `CDP`.

8. CP14 DCC.

9. CP15 absent tie-off and optional external system-coprocessor attachment.

## Milestone 7 — Debug

1. Debug state machine.

2. `DBGEN`.

3. `DBGRQ`.

4. `DBGBREAK`.

5. `DBGACK`.

6. Breakpoint entry.

7. Watchpoint entry.

8. Monitor mode.

9. EmbeddedICE-RT registers.

10. Watchpoint units.

11. DCC through JTAG.

12. Debug PC behavior.

## Milestone 8 — JTAG/TAP

1. TAP controller.

2. Public JTAG instructions.

3. IDCODE.

4. Scan chain 1.

5. Scan chain 2.

6. EmbeddedICE programming through scan chain.

7. DCC through scan chain.

## Milestone 9 — ETM-facing interface

1. Trace-visible signals.

2. `DBGINSTRVALID`.

3. `DBGnEXEC`.

4. ETM pin mapping wrapper.

5. Trace tests.

## Milestone 10 — Cleanup and signoff

1. Full regression.

2. Random instruction testing.

3. Random interrupt/abort/debug testing.

4. Lint.

5. Synthesis.

6. Timing closure.

7. FPGA bring-up or ASIC-style synthesis.

8. Documentation.

---

# 29. Final “complete chip” checklist

A complete implementation should pass this checklist:

```text
[ ] ARMv4T programmer model implemented
[ ] ARM state implemented
[ ] Thumb state implemented
[ ] ARM/Thumb interworking implemented
[ ] All ARM instruction groups implemented
[ ] All Thumb instruction groups implemented
[ ] All processor modes implemented
[ ] All banked registers implemented
[ ] CPSR/SPSR behavior implemented
[ ] Reset behavior implemented
[ ] Exception vectors implemented
[ ] Exception priority implemented
[ ] IRQ/FIQ implemented
[ ] Data abort implemented
[ ] Prefetch abort implemented
[ ] SWI implemented
[ ] Undefined instruction trap implemented
[ ] 3-stage pipeline implemented
[ ] PC pipeline semantics correct
[ ] Pipeline flush/refill correct
[ ] Memory interface pins implemented
[ ] N/S/I/C cycles implemented
[ ] CLKEN wait-state support implemented
[ ] Endianness implemented
[ ] Byte/halfword/word access implemented
[ ] LOCK/SWP behavior implemented
[ ] DMORE implemented
[ ] Cycle timing matches Chapter 7
[ ] Coprocessor interface implemented
[ ] Coprocessor busy-wait implemented
[ ] Coprocessor absent trap implemented
[ ] CP14 DCC implemented
[ ] CP15/system-control-facing behavior handled
[ ] EmbeddedICE-RT implemented
[ ] Breakpoints implemented
[ ] Watchpoints implemented
[ ] Monitor mode implemented
[ ] Debug halt mode implemented
[ ] Debug status/control/abort registers implemented
[ ] JTAG TAP implemented
[ ] Scan chains implemented
[ ] TAP IDCODE implemented
[ ] ETM-facing interface implemented
[ ] ATPG/scan wrapper implemented if required
[ ] Full architectural tests pass
[ ] Full cycle tests pass
[ ] Synthesis passes
[ ] Timing constraints pass
```

The hardest parts will be **PC/pipeline semantics**, **LDM/STM abort behavior**, **Thumb interworking**, **cycle-accurate memory bus timing**, and **EmbeddedICE-RT/JTAG debug**. A good learning path is to treat the first working CPU as a stepping stone, then iteratively replace simplified behavior with the exact ARM7TDMI-S behavior from the TRM.

---

# 30. TRM gap-fill addenda (post-roadmap audit)

These items tighten or extend the §1–§29 roadmap to match TRM (`ARM_DDI_0234B_ARM7TDMI-S_r4p3_TRM.pdf`, 242 pages) at the level of detail any conformant implementation must hit. Numbering: **§30.N amends §N**. Page numbers refer to the PDF page (the document's printed page numbers differ).

## 30.0 ARMv4T scope guard — features NOT in r4p3

Do not implement; remove if mistakenly introduced:

1. `BKPT` instruction — ARMv5+ only. Software breakpoints on r4p3 are pattern-matched via EmbeddedICE-RT, not via a `BKPT` opcode (TRM §1.2 / §B.1).
2. `BLX` (immediate or register) — ARMv5+. ARM↔Thumb exchange on r4p3 is `BX` only.
3. `CLZ` — ARMv5+.
4. Q (saturation) flag in CPSR — ARMv5E+.
5. ARM7TDMI hard-macrocell pin names absent from r4p3: `MAS[1:0]` (replaced by `SIZE[1:0]`), `ABE/DBE/TBE`, `ALE/APE`, `BL`, bidirectional `D`, `nM`, `BUSDIS/BUSEN/NnENIN/nNENOUT/nENOUTI` (TRM §B.4.6 Table B-2).
6. Plausible-sounding pins that do not exist: `DBGRESTART`, `DBGINSTR`. `RESTART` is a JTAG TAP *instruction* (`4'b0100`); debug-state instruction injection is via scan chain 1 with bit 33 selecting debug-speed vs system-speed (§30.23.6). `DBGINSTRVALID` is real and distinct.

## 30.3 Programmer-visible state (amends §3)

1. **CPSR/SPSR explicit bit map** (TRM §2.8 Fig. 2-6, p. 48):
   - `[31]=N`, `[30]=Z`, `[29]=C`, `[28]=V` — flags.
   - `[27:8]` reserved — preserve under read-modify-write; never assume a value (TRM §2.8.3, p. 50).
   - `[7]=I` (IRQ disable), `[6]=F` (FIQ disable), `[5]=T` (state), `[4:0]=M[4:0]` (mode).
   - SPSRs share the layout. **No Q flag** (ARMv4T).
2. FIQ banks **7** registers (`r8_fiq…r14_fiq` + `SPSR_fiq`); other exception modes bank only `r13_xxx, r14_xxx, SPSR_xxx` (TRM §2.7.1, p. 56).
3. System mode (`M[4:0]=11111`): privileged, shares User register set, no SPSR (TRM §2.6 / Table 2-2).

## 30.4 Reset (amends §4)

1. `nRESET` must be held LOW for ≥ 2 `CLK` cycles. While LOW, the bus is forced into I-cycles; any in-flight instruction abandons on the next non-wait cycle. On rising `nRESET`, fetching restarts at `0x00000000` (TRM Appendix A `nRESET`, p. 222).
2. `CFGBIGEND` is *static*. Tie or hold stable; must not change during debug. To reconfigure endianness, hold `nRESET` LOW (TRM §5.3.4 Note, p. 121).

## 30.5 Datapath primitives (amends §5)

1. **Multiplier early-termination `m` parameter** drives every multiply's cycle count:
   - `m=1` if `Rs[31:8]` are all 0 or all 1.
   - `m=2` if `Rs[31:16]` are all 0 or all 1.
   - `m=3` if `Rs[31:24]` are all 0 or all 1.
   - `m=4` otherwise.
   - `MUL`: `m·I + S`. `MLA`: `(m+1)·I + S`. `MULL/SMULL`: `(m+1)·I + S`. `MLAL/SMLAL`: `(m+2)·I + S` (TRM §7.7 Tables 7-7..7-10, pp. 194–195).
   - `MULL/MLAL` are ARM-state-only; `MUL` is the only Thumb multiply.

## 30.8 PSR transfer (amends §8 / §9)

1. `MSR` field-mask bits (per Table 1-10): `_c`=bit 16 (control), `_x`=bit 17 (extension, RAZ/SBZP), `_s`=bit 18 (status, RAZ/SBZP), `_f`=bit 19 (flags).
2. User mode may write only `_f` of `CPSR`; privileged modes write any field.
3. **`MSR` and the T bit**: TRM §2.8.2 (p. 49) says programs *must not* alter the T bit via `MSR` — behavior is UNPREDICTABLE. Pick a hardware policy (drop the T-bit write, or honor it and let the architectural UNPREDICTABLE bite); document the choice and exercise with a cycle test.

## 30.10 Branch (amends §10)

1. `B`/`BL` cycle order (TRM §7.3 Table 7-3):
   - Cycle 1: prefetch from current PC continues + destination computed (the prefetch already issued — too late to cancel).
   - Cycle 2: N-cycle fetch from branch destination; if `BL`, save `(PC+i)` to `r14`.
   - Cycle 3: S-cycle fetch from `dest+i`; if `BL`, post-decrement `r14` by 4 so `MOV PC, R14` returns to instruction *after* the BL.
2. Thumb `BL` is two halfwords (TRM §7.4 Table 7-4): first executes as a data-op accumulating `PC + upper_offset` into `r14`; second performs the branch with `r14` fix-up of −2.
3. `BX` cycle order (TRM §7.5 Table 7-5):
   - Cycle 1: `ADDR=PC+2i`, T = current state.
   - Cycle 2: `ADDR=pc'`, T = new state. `CPTBIT` reflects the new state from this cycle on.
   - Cycle 3: `ADDR=pc'+i'`. ARM→Thumb: `i=4, i'=2`; Thumb→ARM: `i=2, i'=4`.

## 30.12 Block data transfer (amends §12)

1. **LDM data abort** (TRM §2.9.6, p. 54; §7.10, p. 199):
   - Complete the instruction but suppress all destination-register writes that follow the abort.
   - If the writeback bit is set, the modified base register *is* written (handler sees post-modified base — restore manually if it must be undone).
   - r15 is always last in the list, so any abort prevents PC overwrite.
2. **STM** writeback completes normally on abort; handler must be aware base is updated.
3. **Empty register list** in LDM/STM is architecturally UNPREDICTABLE; pick a behavior (TRM is silent), document it, and lock it in a cycle test.

## 30.13 SWP (amends §13)

1. `LOCK` HIGH for both data cycles of `SWP/SWPB`. Sequence (TRM §7.12 Table 7-15): `N(prefetch) → N(read, LOCK=1) → I(write, LOCK=1) → S(continue prefetch, LOCK=0)`.
2. SWP is **ARM-state only**.
3. On data abort, `SWP` behaves as if it had not executed; the abort must occur on the read access.

## 30.14 Exceptions (amends §14)

1. **Return-address offsets per exception** (TRM §2.9.1 Table 2-3, p. 51):
   - `BL`/`SWI`/`Undef` from ARM ⇒ save `PC+4` to LR; from Thumb ⇒ save `PC+2`.
   - Prefetch Abort ⇒ save `PC+4`; return via `SUBS PC, R14_abt, #4`.
   - Data Abort ⇒ save `PC+8`; return via `SUBS PC, R14_abt, #8`.
   - IRQ/FIQ ⇒ save `PC+4`; return via `SUBS PC, R14_xxx, #4`.
   - Reset: `r14_svc` is UNPREDICTABLE.
2. **Data Abort + FIQ coincidence**: enter the Data Abort handler first, then immediately vector to FIQ at `0x1C`. A naive priority encoder that fires only the highest-priority exception loses the abort (TRM §2.9.10, p. 60).
3. `nFIQ` and `nIRQ` are **synchronous, level-sensitive**, sampled on rising `CLK`. Hold LOW until handler clears the source. Async sources need an external synchronizer — r4p3 has no `ISYNC` pin (TRM Appendix A; §B.4.4).
4. **Worst-case FIQ latency** = `T_syncmax + T_ldm + T_exc + T_fiq = 2 + 20 + 3 + 2 = 27 cycles` (zero-wait-state, longest LDM including PC). Min FIQ/IRQ latency = 4 cycles. Verify in §27 (TRM §2.10, p. 60).

## 30.17 Memory interface (amends §17)

1. **Von Neumann bus** — instruction and data share `ADDR/RDATA/WDATA`. The pipeline arbitrates one bus cycle for either fetch or data, never both (TRM §1.1.2, p. 19).
2. **Burst rules** (TRM §3.3, pp. 77–79):
   - Bursts are word (+4 per S) or halfword (+2 per S) only. **Byte bursts do not exist.**
   - Within a burst, `WRITE/SIZE/PROT/LOCK` hold constant; only `ADDR` advances.
   - A burst opens with N (or merged I-S); subsequent beats are S.
   - During an I cycle the address of the next sequential access is broadcast for early decode, but the memory controller must not commit during the I cycle (an I may be followed by an N to an unrelated address on PC writes/exceptions).
3. **`SIZE` encoding** (TRM §3.4.3 Table 3-3): `00=byte`, `01=halfword`, `10=word`, `11=reserved` (never generated; assert).
4. **Significant-address-bit rule** (TRM §3.5.4 Table 3-6): word — use `ADDR[31:2]`; halfword — `ADDR[31:1]`; byte — `ADDR[31:0]`.
5. **`ABORT` sampling** (TRM §3.5.3, p. 87): sampled on rising `CLK` *only during S/N memory cycles*; ignored in I/C. On data access, traps immediately. On opcode fetch, marks the prefetched instruction invalid; trap fires only when (and if) it reaches Execute.
6. **`DMORE`** (TRM §1.5.5, p. 40 / Appendix A): an address-class
   prediction, HIGH when the next data memory access is followed by a
   sequential data memory access. For an n-word LDM/STM it is HIGH on data
   addresses 1 through n-1 and LOW on address n. It is sampled with
   `ADDR/TRANS`, not with the returning `RDATA/WDATA` phase.
7. During `nRESET` LOW the bus is forced into I-cycles; existing memory cycles abandon. No spurious access on the way to `0x00000000`.

## 30.18 Cycle accuracy (amends §18)

Anchor each cycle test to an explicit TRM table:

1. **Data-op** (Table 7-6, p. 194): `+S` (1S); shifted-by-register form adds an I.
2. **MSR/MRS** (Table 7-6).
3. **LDR** (Table 7-11, p. 196): `+N+I+S`; if `Rd=PC`, add `+N+S`.
4. **STR** (Table 7-12, p. 196): `+N+N`.
5. **LDM** (Table 7-13, p. 196):
   - 1 reg, dest≠PC: `+N+I+S`.
   - 1 reg, dest=PC: `+N+I+N+2S`.
   - n>1, dest≠PC: `+N+(n−1)S+I+S`.
   - n>1, dest=PC: `+N+(n−1)S+I+N+2S`.
6. **STM** (Table 7-14, p. 197):
   - Chapter 7's summary table (Table 7-2) gives
     `+N+(n−1)S+I+N`.
   - Table 7-14's externally visible address/control rows show `N,N` for one register
     and `N,(n−1)S,N` for multiple registers, while its prose calls the one-register
     case two cycles. Treat this as a specification-reconciliation item: document how
     the internal I operation is represented or overlapped and verify both architectural
     execution time and every external bus row. Do not infer conformance from an
     `n+1` E-state counter alone.
7. **SWP** (Table 7-15): per §30.13.
8. **Exception entry** (Table 7-16, p. 201): `+N+2S` total (cycle 1 N to `PC+2i` in old mode; cycle 2 S to vector `Xn` in new mode/Tbit; cycle 3 S to `Xn+4`).
9. **B/BL** (Table 7-3): `+N+2S`.
10. **BX** (Table 7-5): `+N+2S` with Tbit transitions per §30.10.3.
11. **MUL/MLA/MULL/MLAL** (Tables 7-7..7-10): per §30.5.1.
12. **CDP** (Table 7-17, p. 201): `+S` plus busy-wait I cycles if `CPB` HIGH.
13. **LDC/STC** (Table 7-2; Tables 7-18 / 7-19):
    `+(b)I+N+(n−1)S+N`, with the coprocessor determining the transfer length.
14. **MCR/MRC** (Table 7-2; Tables 7-20 / 7-21):
    `+(b)I+C+N` (`MCR`) and `+(b)I+C+I+S` (`MRC`). Verify the C cycle,
    busy-wait cycles, data phase, and following fetch separately.
15. **Undef / coprocessor-absent trap** (Table 7-22): same path as exception entry.
16. **Unexecuted instruction** (Table 7-23, p. 204): exactly `+S` at `PC+2i`. All side effects suppressed except the bus cycle and `DBGnEXEC` HIGH. Easy smoke test.

## 30.19 Coprocessor interface (amends §19)

1. **Reserved CP IDs** (TRM §4.1.1 Table 4-1): CP15=system control, CP14=DCC + Abort Status Register, CP13:8=ARM-reserved, CP7:4=user-available, CP3:0=ARM-reserved. External coprocessors must use CP4–CP7.
2. **Pipeline-following** (TRM §4.3, p. 93): coprocessors mirror the core. Load into pipeline on rising `CLK` only when `CPnOPC=0 ∧ CPnMREQ=0 ∧ CPTBIT=0` were *all* LOW in the *previous* bus cycle (= ARM-state opcode fetch). Advance pipeline when the same triple is LOW in the *current* cycle. Coprocessor state changes only when `CLKEN` HIGH (except reset).
3. **Privilege via `CPnTRANS`** (TRM §4.8 Table 4-4): `CPnTRANS=0`=User, `=1`=privileged. Coprocessor checks at decode; User-mode access to a privileged-only operation is rejected with `CPA=1, CPB=1` (absent), forcing the undef trap.
4. **Busy-wait abandonment** (TRM §4.4.4, p. 96; §5.3.3, p. 121): on a non-masked IRQ/FIQ or `DBGRQ`, the core takes `CPnI` HIGH. Coprocessor monitors `CPnI` and abandons. Coprocessor actions during busy-wait must be *idempotent* — must not corrupt state and must repeat with identical results.
5. **No-coprocessor tie-off** (TRM §4.6, p. 102; Table 4-3): tie `CPA=1, CPB=1`; leave outputs `CPnMREQ, CPSEQ, CPnTRANS, CPnOPC, CPnI, CPTBIT` unconnected. All coprocessor instructions then take the undef trap. For multiple coprocessors: AND each one's `CPA` together, AND each one's `CPB` together; common `CPnI` fans out.
6. **`CPn`-prefix polarity reminder**: active-LOW (`CPnOPC=0` ⇒ opcode fetch). `CPSEQ`, `CPA`, `CPB` are active-HIGH.
7. LDC/STC may transfer >16 words; this stretches worst-case interrupt latency past 27 cycles. Recommend a 16-word software cap.

## 30.20 CP14 / DCC (amends §20)

1. **CP14 register set**:
   - DCC control register (32-bit): `[31:28]=4'b0111` for an exact r4p3
     implementation. DDI 0234B prints `4'b0001`, but ARM's ARM7TDMI-S Errata List
     (FR002-PRDC-002719 7.0, erratum [9]) says the Rev-4 EmbeddedICE-RT version is
     7 and that r4p3 corrected the JTAG view. `[27:2]` are reserved. `[1]=W`:
     W=0 means the processor-to-debugger transmit buffer is empty and the processor
     may write; W=1 means data is pending and the debugger may scan it out. `[0]=R`:
     R=0 means the debugger-to-processor receive buffer is empty and the debugger may
     write; R=1 means unread data is present and the processor may read it.
   - DCC data-read / data-write register.
   - **Abort Status Register (1 bit `DbgAbt`)** — accessed via `MRC/MCR CP14`. Set when monitor mode causes a Prefetch/Data Abort due to a breakpoint or watchpoint; if both an external `ABORT` and a debug abort coincide, external `ABORT` wins and `DbgAbt` stays 0. Sticky; cleared by software write (TRM §5.23, p. 161).
2. **DCC instructions**:
   - `MRC CP14,0,Rd,C0,C0` — read DCC control.
   - `MRC CP14,0,Rd,C1,C0` — read DCC data-read.
   - `MCR CP14,0,Rn,C1,C0` — write DCC data-write.
3. **Rev 4 single-access optimization** (TRM §5.10.2, pp. 134–135): DCC data and status fit in one scan-chain-2 access; status bit is the LSB of the address field returned. DCC control bit 0 preserved for backward compatibility.
4. **`DBGCOMMRX/TX` polarity** (Appendix A): `DBGCOMMRX=1` ⇒ rx buffer full; `DBGCOMMTX=1` ⇒ tx buffer empty. Both gated by `DBGEN=1`.
5. Coprocessor instructions, **including DCC `MCR/MRC` to CP14, do not exist in Thumb**. Monitor-mode debugger code reaching DCC from Thumb must switch to ARM first.

## 30.22 EmbeddedICE-RT (amends §22)

1. **`DBGEN=0`**: `DBGBREAK/DBGRQ` ignored; `DBGACK` forced LOW; interrupts pass through; macrocell enters low-power. **Caution from TRM**: tying `DBGEN` LOW is not a security mechanism — implement security elsewhere if required (TRM §5.7, p. 124).
2. **Watchpoint mask is XNOR, not AND** (TRM §5.20.2 / §5.26):
   - `match = ((value XNOR input) OR mask) == all-1s`.
   - Mask bit = 1 ⇒ that position always matches; mask bit = 0 ⇒ exact equality required. Common implementation bug is to AND.
3. **Watchpoint coupling** (TRM §5.20.3 / §5.26.1, pp. 169–170):
   - `CHAIN`: WP1's `CHAINOUT` feeds WP0's `CHAIN`. WP1 has no CHAIN input; WP0 has no CHAINOUT.
   - `CHAINOUT` is a latch: write-enable from WP1's address/control comparator, D-input from WP1's data comparator. Cleared on control-value-register write or `DBGnTRST=0`.
   - `RANGE`: WP1's `RANGEOUT` feeds WP0's `RANGE`. Power-of-2 ranges only.
   - `DBGRNG[1:0]` external pins reflect each watchpoint's address/control + data-mask match **independent of the ENABLE bit** (so trace can see range hits without breaking).
   - ENABLE = bit 8 of the 9-bit control-value register; **unmaskable** (no mask-register equivalent for bit 8).
4. **Debug Control Register — 6 bits** (TRM §5.24 Table 5-7, p. 161):
   - `[5]` EmbeddedICE-RT disable (Rev 4).
   - `[4]` Monitor-mode enable (Rev 4 — abort vs halt).
   - `[3]` SBZ/RAZ.
   - `[2]` `INTDIS` (force `IFEN` LOW; mask interrupts even outside debug).
   - `[1]` force-`DBGRQ`.
   - `[0]` force-`DBGACK`.
5. **Debug Status Register — 5 bits, read-only** (TRM §5.25, p. 164):
   - `[4]=TBIT, [3]=TRANS[1], [2]=IFEN, [1]=synced DBGRQ, [0]=synced DBGACK`.
6. **Interrupt-mask logic** (TRM Fig. 5-17, p. 165): `IFEN_to_core = !((bit2 OR DBGACKI) | INTDIS)`. `DBGACK_pin = bit0 OR DBGACKI`. `DBGRQI_to_core = bit1 OR sync(DBGRQ_input)`.
7. **Reprogramming sequence under monitor mode** (TRM §5.9.2, p. 130):
   1. Set debug-control bit 5 (disable).
   2. **Poll** bit 5 read-back until set.
   3. Modify watchpoint registers and/or bit 4.
   4. Clear bit 5.
   - Poll step is mandatory — propagation is async; skipping it causes false matches.
8. **Software breakpoints** (TRM §5.21.2, p. 150):
   - Program a watchpoint with a magic data-value pattern and `0xFFFFFFFF` address mask.
   - For Thumb, **replicate** the 16-bit pattern in both halves of the 32-bit data-value register (a `LDR` may return either halfword on the 32-bit bus).
   - Architectural reservations for breakpoint encodings: ARM `0xE7Fxxxxx`, Thumb `0xDExx/0xBExx`. (No `BKPT` opcode on r4p3 — see §30.0.)
9. **Breakpoint flushed by branch/exception** (TRM §5.3.1, p. 120): if the instruction is flushed (preceding branch / exception / PC write), debug entry is canceled. On exception return + refetch, the breakpoint reflags. Comparator's ENABLE qualification must combine with a pipeline-valid signal.
10. **Watchpoint completion** (TRM §5.3.2 / §5.18.3): fires *after* the access completes (data writeback / base writeback finished). For LDM/STM, many cycles can elapse between match and entry. Watchpointed access that also Data Aborts ⇒ debug state entered in abort mode (vector fetch first). Watchpointed access coincident with another exception ⇒ debug state entered in that exception's mode; debugger disambiguates via CPSR/SPSR/PC.
11. **Monitor-mode restrictions** (TRM §5.9.2): in monitor mode,
    breakpoints/watchpoints **cannot** be data-dependent or RANGE/CHAIN-coupled.
    `DBGEXT[0]` and `DBGEXT[1]` are permitted qualifiers and are explicitly listed
    among the supported monitor conditions. External `DBGBREAK` is not supported.
    Halt and monitor modes cannot mix.
12. **IFEN auto-disable in debug state** (TRM §5.19.2, p. 147): on debug-state entry, IRQ and FIQ are forced disabled internally regardless of `CPSR.I/F`. Pending interrupts at entry are remembered.
13. **Debug-exit return-PC offsets** (TRM §5.18.6, p. 146):
   - Normal break/watch exit: `−(4 + N + 3S)`, with N = debug-speed, S = system-speed.
   - DBGRQ-entry / watchpoint-with-exception: `−(3 + N + 3S)`.
   - System-speed access drops `DBGACK` temporarily; force via control-bit 0 if peripherals must be held inhibited.
14. **Allowable debug-state instructions** (TRM §5.16.1, p. 162): only data-processing, all loads/stores including LDM/STM, and MSR/MRS may be scanned in. STM is the standard register-file dump path. Mode changes between any two modes are allowed in debug state.

## 30.23 JTAG TAP and scan chains (amends §23)

1. **Public JTAG instructions — exactly five** (TRM §5.13 Table 5-3, p. 153):
   `SCAN_N=4'b0010`, `INTEST=4'b1100`, `IDCODE=4'b1110`, `BYPASS=4'b1111`, `RESTART=4'b0100`. Other 4-bit encodings default to `BYPASS`.
2. **Do NOT implement** `EXTEST/SAMPLE/PRELOAD/CLAMP/HIGHZ/CLAMPZ` — there is no boundary-scan chain on r4p3. Selecting them while scan chain 1 or 2 is active yields UNPREDICTABLE behavior (TRM §B.4.3, p. 233).
3. **IR width = 4 bits**, no parity. CAPTURE-IR loads fixed pattern `4'b0001` (LSB-first). SHIFT-IR shifts in LSB-first. UPDATE-IR latches. Reset value = `IDCODE` (TRM §5.14.3, p. 156).
4. **IDCODE** (TRM §5.14.2, p. 155):
   - 32-bit format `[31:28]=Version, [27:12]=PartNumber, [11:1]=ManufacturerID, [0]=1` (LSB always 1 per IEEE 1149.1; do not tie LOW).
   - r4p3 default value `0x7F1F0F0F` (matches `AGENTS.md`).
   - No parallel write. CAPTURE-DR loads the constant; SHIFT-DR clocks LSB-first; UPDATE-DR is a no-op for IDCODE.
5. **Scan chain map**:
   - 0: reserved (returns zeros if selected).
   - 1: debug, **33 bits** = `[31:0]` data + bit 33 (= `[32]`) `DBGBREAK` cell.
   - 2: EmbeddedICE-RT, **38 bits** = `[37]` R/W + `[36:32]` 5-bit reg-addr + `[31:0]` data; commit on UPDATE-DR.
   - 3, 4, 8: reserved.
6. **Scan chain 1 bit-33 dual semantics** (TRM §5.14.5 / §5.16):
   - During INTEST: scans a known value into the `DBGBREAK` input.
   - During debug-state instruction injection: bit 33 = 0 ⇒ run at debug speed; bit 33 = 1 ⇒ run at system speed (sync to `CLKEN`).
   - On debug-state entry capture: bit 33 = 0 ⇒ entered from breakpoint; bit 33 = 1 ⇒ from watchpoint.
   - For a system-speed access: bit 33 HIGH on the penultimate scanned instruction; the final branch has bit 33 LOW. After RESTART, the core executes at full speed via `CLKEN` then re-enters debug state.
7. **TAP reset via `DBGnTRST=0`** (TRM §5.3.5 Fig. 5-4, p. 117; §5.12.1, p. 152):
   - TAP state machine → Test-Logic-Reset.
   - Current instruction → `IDCODE`.
   - Active scan chain → 0.
   - All EmbeddedICE-RT D-types in Fig. 5-4 cleared (TCK synchronizer, TMS/TDI sample-hold).
   - `CHAINOUT` cleared.
   - No `CLK`/`DBGTCKEN` pulse needed.
   - For JTAG use, `DBGnTRST` must pulse LOW then HIGH at power-on; tie LOW permanently if JTAG unused.
8. **`DBGnTDOEN` polarity** (Appendix A): LOW = TDO actively driven; HIGH = HiZ. Gated by `DBGEN`.
9. **3-stage TCK synchronizer** to `CLK` for off-chip Multi-ICE-style debug (TRM Fig. 5-4); off-chip device must wait on `RTCK` before the next TCK edge. TMS/TDI on-chip latches gated by `DBGTCKEN`. All synchronizer flops reset by `DBGnTRST`.
10. **Scan-path-select register**: 4 bits; CAPTURE-DR loads `4'b1000`; UPDATE-DR latches selection. Default chain 0 on TAP reset.
11. **INTEST wire order is chain-specific** (TRM §5.11.1/§5.14.5):
   - Chain 1 physically runs TDI → `DATA[0]` … `DATA[31]` → `DBGBREAK` → TDO.
     `DBGBREAK` is therefore the first captured bit shifted out; a complete host
     load sends `DBGBREAK` followed by `DATA[31:0]` most-significant bit first.
   - Chain 2 physically runs TDI → R/W → `ADDR[4:0]` (4 to 0) →
     `DATA[0:31]` → TDO. Do not serialize either INTEST chain as a generic packed
     LSB-first vector; only IR, IDCODE, and SCAN_N have that stated ordering.

## 30.24 ETM-facing interface (amends §24)

1. **`DBGINSTRVALID`** = HIGH for exactly one cycle per instruction *committed* to Execute. Used by ETM7 to count executed instructions, **not** bus accesses (Appendix A).
2. **Combination rule**: instruction committed = `DBGINSTRVALID && !DBGnEXEC`. `DBGnEXEC` HIGH means an instruction reached Execute but failed its condition codes (no commit).
3. **PROCID tie-off**: r4p3 has no OS context-ID source. When wrapping ETM7, tie `PROCID[31:0]=0` and `PROCIDWR=0` at the ETM input (TRM §6.3 Table 6-1 footnotes, p. 175).
4. **Direct RDATA/WDATA exposure**: connect `RDATA[31:0]` and `WDATA[31:0]` straight to the ETM so coprocessor cycles can be traced (TRM §4.5.1 Note, p. 100).
5. **Reset propagation**: connect system `nTRST`→`DBGnTRST` so `PWRDOWN` and ETM state reset in lockstep with the macrocell (TRM §6.4, p. 176).

## 30.26 AC / timing (amends §26)

1. Disposition every TRM Table 8-1 percentage-of-`CLK` budget when a target
   toolchain and physical boundary are chosen (TRM §8.2, pp. 216–217).
   Table 8-1 is explicitly provisional synthesized-macrocell/silicon AC
   guidance, not a portable promise that can be copied unchanged onto
   soft-core FPGA pins. A raw
   compatibility profile must either translate each applicable percentage
   into target-specific SDC/STA checks or document why the enclosing
   synchronous FPGA boundary replaces that pin-level parameter. It must never
   silently omit the row or advertise a portable pin-timing guarantee. The source
   percentages include:
   - **Setup**: `CLKEN` 40%, `ABORT` 15%, `RDATA` 10%, `CPA/CPB` 20%, `nFIQ/nIRQ/nRESET` 10%, `CFGBIGEND` 10%, `DBGTCKEN` 40%, `DBGTDI/DBGTMS` 35%.
   - **Hold**: 0% on most inputs (clock-skew budget only).
   - **Output valid (max % of t_cyc)**: `ADDR` 90%, control (`WRITE/SIZE/PROT/LOCK`) 90%, `TRANS` 50%, `WDATA` 40%, `DBGTDO` 20%, debug control 40%, debug status 40%, coprocessor control 80%.
2. Off-chip JTAG path needs the 3-stage TCK synchronizer (§30.23.9) and the `RTCK`-handshake convention.

## 30.27 Conformance tests (amends §27)

1. Each cycle test cites a specific TRM table (§30.18). Regressions stay traceable.
2. Worst-case-FIQ-latency cycle test targeting 27 cycles (§30.14.4).
3. CHAIN/RANGE coupling reference cases (TRM §5.26):
   - "Break on address Y only when process X is running" (CHAIN: WP1 matches process-ID location with `data=X` ⇒ `CHAINOUT` enables WP0).
   - "Break on first 256 bytes but not first 32" (RANGE: WP1 disabled, mask `0x1F`; WP0 enabled, mask `0xFF`, RANGE bit cleared).
4. `MSR` T-bit-write policy test — verify the implementation matches its documented choice (drop vs honor; §30.8.3).
5. Software-breakpoint replication test for Thumb — verify both halfwords of the 32-bit data-value register match.
6. `ABORT`-during-I-cycle ignored — explicit negative test.
7. Unexecuted-instruction `+S` cycle (§30.18.16) as a smoke test.

## 30.29 Final checklist additions (amends §29)

```text
[ ] Exception-entry return-address offsets per Table 2-3 verified
[ ] LDM abort writeback + r15-protect verified
[ ] SWP LOCK held across both data cycles
[ ] FIQ-after-Data-Abort interlock verified
[ ] Worst-case 27-cycle FIQ latency verified
[ ] CFGBIGEND tied static; not changed mid-debug
[ ] nRESET ≥ 2 cycle hold modeled in TB
[x] CPSR/SPSR explicit bit map (no Q flag) implemented
[x] MSR T-bit-write policy chosen, documented, and tested
[ ] Multiplier early-termination m parameter verified per Tables 7-7..7-10
[ ] ABORT sampled only on S/N cycles
[ ] Burst rules (constant control, +4/+2 ADDR, no byte burst) verified
[ ] DMORE distinct from TRANS verified
[ ] CP14 Abort Status Register implemented and DbgAbt sticky
[ ] DCC Rev-4 single-access optimization in scan chain 2
[ ] Watchpoint mask is XNOR (not AND)
[ ] CHAINOUT latch + clear on CV-write / DBGnTRST
[ ] DBGRNG independent of ENABLE bit
[ ] Debug control register = 6 bits per Table 5-7
[ ] Debug status register = 5 bits per §5.25
[ ] Reprogramming poll sequence (set bit5 → poll → modify → clear) implemented
[ ] Scan chain 1 = 33 bits with bit-33 dual semantics
[ ] Scan chain 2 = 38 bits committed on UPDATE-DR
[ ] Public JTAG instruction set = exactly 5 (others default to BYPASS)
[ ] EXTEST/SAMPLE/PRELOAD/CLAMP/HIGHZ not implemented
[ ] IDCODE LSB tied 1, value 0x7F1F0F0F
[ ] DBGnTRST resets TAP / instruction / chain-select / CHAINOUT / sync flops
[ ] DBGINSTRVALID + !DBGnEXEC = committed instruction (ETM)
[ ] PROCID/PROCIDWR tied LOW at ETM wrapper
[ ] No BKPT / BLX / CLZ / Q / MAS / DBGRESTART / DBGINSTR introduced
```

---

# 31. Release-readiness audit and immutable sign-off contract

This section answers two different questions that the earlier roadmap mixed together:

1. Does source code for a feature exist?
2. Is that feature correct, interoperable, synthesized, and proven well enough to ship?

At the audited baseline, the answer to the first question is often "partly"; the answer
to the second is **no**. The repository is a useful prototype, but it is not yet a
conformant ARM7TDMI-S replacement or drop-in MiSTer component. This section is the
authoritative backlog for changing that answer.

## 31.0 What "complete" and "drop-in" mean

The v1.0 release has two required, versioned integration surfaces:

1. **`arm7tdmis_top` conformance profile** — an ARM7TDMI-S r4p3-compatible soft
   macrocell: ARMv4T programmer's model, documented r4p3 cycle behavior, memory bus,
   external coprocessor protocol, CP14 DCC, EmbeddedICE-RT, JTAG scan chains, and the
   documented ETM-facing signals. CP15 is not built into a bare ARM7TDMI-S; an external
   system coprocessor may claim p15 through the coprocessor interface.
2. **`arm7tdmi_mister` integration profile** — a stable FPGA wrapper around the CPU
   with a conventional request/completion memory interface, MiSTer-style clock-enable
   operation, reset/interrupt synchronization contract, optional debug tie-offs,
   complete file manifest, timing constraints, and save/restore support. A
   PocketStation reference integration and one generic SoC example prove the contract.

The FPGA profile may compile out ASIC production-test scan logic and an ETM macrocell,
but it must expose honest tie-offs and document that those blocks are **OUT OF SCOPE**.
Merely exposing `SE/SI/SO` while tying `SO` low is not an implementation. The ETM7
macrocell itself is not required; all signals that the r4p3 core promises to an external
ETM are required and tested.

"Drop-in" means compatible with those two published interfaces. It cannot mean
compatible with every unknown future core without an adapter. After v1.0 sign-off, this
ledger is frozen; a new consumer requirement opens a versioned v1.x/v2 task instead of
retroactively invalidating v1.0.

### Authoritative source hierarchy

Before changing behavior, freeze exact copies and SHA-256 hashes of:

1. [ARM Architecture Reference Manual ARM DDI 0100I](https://documentation-service.arm.com/static/5f8dacc8f86e16515cdb865a),
   restricted to ARMv4T behavior.
2. The in-repository [ARM7TDMI-S r4p3 TRM](ARM_DDI_0234B_ARM7TDMI-S_r4p3_TRM.pdf),
   ARM DDI 0234B.
3. [ARM7TDMI-S Errata List FR002-PRDC-002719 7.0](https://documentation-service.arm.com/static/5ed62dc5ca06a95ce53f9214)
   (20 March 2009).
4. [ARM Application Note 99, ARM DAI 0099C](https://documentation-service.arm.com/static/5ed1094dca06a95ce53f8a9d),
   for CP15 and TAP identification.
5. The applicable IEEE 1149.1 revision for TAP behavior.
6. The versioned MiSTer wrapper specification created by §31.9.
7. The PocketStation hardware references selected by the reference integration.

The ARM ARM defines architectural instruction behavior; the r4p3 TRM defines this
implementation's pins, pipeline, timing, and debug; the errata list supersedes known
errors and records real-r4p3 deviations. Any conflict gets a written decision and a
directed test. No test may cite README prose as its specification.

The default release implements architecturally corrected behavior. Maintain an errata
matrix that says whether each real-r4p3 defect is corrected or deliberately reproduced;
if software compatibility requires a defect, put it behind an explicit synthesis
parameter and test both settings.

## 31.1 Audited baseline status

| Area | Status at `23bb86a` | Audit evidence / reason |
|---|---|---|
| Synthesizable source structure | IMPLEMENTED, UNVERIFIED | RTL elaborates and Verilator lint completes, but there is no synthesis result. |
| ARM decode/execute | PARTIAL | Broad class coverage exists; flag, PC, PSR, alignment, translated-transfer, reserved-encoding, and abort defects remain. |
| Thumb decode/execute | PARTIAL | A decoder for 19 formats exists, but most formats lack integration coverage and edge encodings are wrong or untested. |
| Modes and banked registers | IMPLEMENTED, UNVERIFIED | Unit tests cover ordinary banking, not every exception/debug/user-bank interaction. |
| Exceptions and interrupts | INCORRECT | LR offsets, simultaneous priority/interlock, and block-abort behavior conflict with DDI 0234B. |
| Memory bus and cycle accuracy | INCORRECT | N/S/I/C waveforms are not checked; endianness is unused; several timing paths do not match the tables. |
| Multiplier | PARTIAL | Arithmetic and some timing paths exist; flag commit and full m/operand/unpredictable space are not proven. |
| External coprocessor interface | INCORRECT | `CPB` is unused, accepted operations have no transfer body, `CPnTRANS` has the wrong meaning, and pipeline-follow timing is not implemented. |
| CP14 DCC | INCORRECT | c0 is used as a shared data register; real c0 control/c1 RX/TX semantics, handshakes, status pins, and `DbgAbt` are absent. |
| CP15 | INCORRECT / EXTRA | A fabricated internal Main ID is returned by an overly broad decode even though a bare ARM7TDMI-S has no CP15. |
| EmbeddedICE-RT | PARTIAL | The RTL calls itself a scaffold; monitor mode, phase-correct data watchpoints, debug-abort handling, and conformant debug execution are absent. |
| JTAG | PARTIAL | A TAP skeleton and basic IDCODE/BYPASS tests exist; scan-chain semantics, DBGEN gating, synchronization, and end-to-end debug are incomplete. |
| ETM-facing behavior | PARTIAL | Two pins exist; commit semantics, pipeline-follow information, reset/tie-offs, and an ETM-facing checker are absent. |
| DFT wrapper | MISSING | `SO` is tied low and `SE/SI` are unused. It must be removed from the FPGA claim or implemented for a separate ASIC profile. |
| Unit verification | PARTIAL | Ten tests run, but `ice_rt_tb` and `jtag_tap_tb` only print failures and still exit successfully. |
| Integration verification | NOT TRUSTWORTHY / CURRENTLY FAILING | All directed benches print failures then call `$finish`; `make` can report success after a failed check. On 2026-07-28, `make unit integ run` exited 0 while smoke printed two failures: SWPB result expected `0x05`, got `0x0d`, and committed PC expected `0x9c`, got `0x96`. The smoke bench is not in `make integ`. |
| Architectural/cycle conformance | MISSING | No independent reference model, full encoding matrix, bus-cycle scoreboard, coverage closure, formal proof, or real conformance suite. |
| Quartus / FPGA closure | MISSING | No QSF/QIP/IP manifest, synthesis, fit, resource, Fmax, CDC/RDC, or hardware result. The SDC names a nonexistent `DBGTCK` port and false-paths synchronous IRQ/FIQ pins. |
| MiSTer integration | MISSING | No MiSTer wrapper, request/completion bridge, build, save-state interface, PocketStation boot test, or generic integration example. |
| Documentation | INCORRECT | README/AGENTS/docs call scaffolded or untested paths "complete" and document incorrect abort/cycle/CP behavior. |

The useful accomplishments are therefore narrower than the old status pages claim:
the repository has a substantial synthesizable RTL prototype, broad decoder/datapath
scaffolding, a pin-oriented top level, ten unit benches, fifteen directed benches plus
a separate smoke bench, and a runnable Verilator flow. Those are foundations, not
conformance evidence.

## 31.2 P0 — make the evidence fail hard

No functional claim can become VERIFIED until every item here is complete.

- [x] **VER-001:** Replace every error-counter-only test ending with `$finish` with
  `$fatal(1, ...)` on any error and `$finish` only on success. This includes every
  integration bench, the smoke bench, `ice_rt_tb`, and `jtag_tap_tb`. Every
  manifest-listed unit, integration, and smoke top now has a fatal failure path.
- [x] **VER-002:** Add a bounded timeout that fails nonzero to every bench.
  Every manifest-listed bench has an independent `$fatal` timeout.
- [x] **VER-003:** Make one `regress` target run RTL lint, TB lint, all unit tests, all
  directed integration tests, and the smoke test from a clean build. The
  `scripts/Makefile` manifests exactly match every unit and directed integration
  test top, and `make -C scripts regress` orders clean, both lint passes, unit,
  integration, and smoke execution.
- [x] **VER-004:** Make stale cached binaries impossible to mistake for a new result;
  record the RTL git hash, tool versions, build variant, seed, and test manifest in the
  regression output. The Rust `verification/constrained_random_rs/src/regression.rs`
  runner starts every full/quick run
  with `clean`, records the Git commit and dirty-source SHA-256, tool/platform
  versions, variant, seed, and complete manifest, then hashes every phase log in
  the atomic `arm7tdmis-regression-v1` JSON result.
- [x] **VER-005:** Run a harness self-test in CI that deliberately corrupts one expected
  result and proves the overall command fails. Add periodic mutation testing for major
  writeback, flag, exception, and bus controls. The local regression and CI run
  `tb/harness/expected_failure_tb.sv` and pass only when its real simulator
  `$fatal` propagates nonzero. The Rust `mutation-self-test` command also requires four
  opt-in RTL mutants to fail through their intended architectural diagnostics;
  a surviving mutant, compile failure, or unrelated test failure fails the
  mutation command. GitHub Actions runs this suite weekly and on manual dispatch.
- [x] **VER-006:** Treat assertions and simulator errors as fatal. Remove unsupported
  warning suppressions or give each one an owner, rationale, and expiry.
  Every compile uses fatal `-Wall`, every simulation enables `--assert`, and
  no RTL/test source uses `$error`. The stale blanket `UNOPTFLAT` waiver was
  removed. `docs/WARNING_POLICY.md` owns the two command-line and three inline
  classes with rationale and expiry conditions; `verification/constrained_random_rs/src/hdl_source_contract.rs`
  mechanically rejects new classes, unpaired pragmas, or weakened fatal flags.
- [x] **VER-007:** Stop treating `ABORT` during I/C as illegal testbench stimulus.
  Assert that the core ignores it there, as the TRM requires.
  `tb/integration/arm7tdmis_abort_inactive_tb.sv` injects ABORT during I/C
  phases, while `tb/integration/arm7tdmis_abort_clken_tb.sv` covers an inactive
  assertion overlapping a stopped clock-enable interval.
- [x] **VER-008:** Publish machine-readable regression and coverage reports as release
  artifacts. A console line containing `PASS` is not release evidence. The
  versioned regression JSON schema and per-phase log hashes are implemented and
  documented in `docs/VERIFICATION.md`. `make -C scripts coverage` now preserves
  four named raw databases, a merged raw database, LCOV, exact test/tool/source
  identity, and an atomic `arm7tdmis-coverage-v1` summary. The evidence packager
  rejects failed, dirty, or cross-commit reports and hashes every report, phase
  log, and coverage file into a commit-addressed archive. GitHub Actions uploads
  those inspectable files and the archive only after validation. This measured
  subset is explicitly not VAL-006 functional-coverage closure.
- [x] **VER-009:** Expose a verification-only architectural retirement interface
  (instruction PC/opcode/state/condition result/register and CPSR effects/exception)
  instead of making all scoreboards depend on private hierarchy. Defining
  `ARM7TDMIS_VERIFICATION` adds the stable `VER_RETIRE_*` top-level contract;
  ordinary RTL/FPGA builds contain no instrumentation ports or logic.
  `arm7tdmis_retire_interface_tb.sv` checks eleven exact events across ARM,
  Thumb, condition failure, load/store substates, BX, and SWI post-state without
  reading private CPU hierarchy.
- [x] **VER-010:** Minimize and resolve both currently failing smoke checks. The SWPB
  check may itself be stale because later test code writes r12=13; decide RTL versus
  expectation only from an instruction trace and cited architecture behavior. Likewise,
  explain/fix the final `pc_q=0x96` versus expected `0x9c`. Preserve each real defect as
  a small fail-hard regression. `tb/integration/arm7tdmis_swpb_data_tb.sv`
  isolates SWPB read data, lane placement, and LOCK lifetime and proved the old
  r12 expectation stale. `tb/integration/arm7tdmis_bx_interwork_tb.sv`
  isolated the odd-address Thumb-to-ARM return defect; destination-state refill
  fixed the final self-loop PC. The original smoke bench now passes fail-hard.

## 31.3 P0 — ARMv4T architectural correctness

Build a requirements/encoding matrix covering every valid ARM and Thumb encoding,
every reserved hole, every condition, register choice, shift amount, addressing mode,
and defined boundary value. Each row links ARM ARM text to RTL and at least one test.

- [x] **ISA-001:** Fix immediate `LSR #0` and `ASR #0` to mean shift by 32, and verify
  `ROR #0` as RRX. Cover immediate and register-specified 0/1/31/32/>32 amounts and
  carry behavior in both states. `tb/unit/shifter_tb.sv` exhausts every boundary
  amount and both carry inputs through the shared ARM/Thumb datapath;
  `tb/unit/decoder_tb.sv` proves the three ARM encoded-zero cases and the distinct
  register-shift-zero path; `tb/unit/thumb_decoder_tb.sv` proves Thumb immediate
  LSR/ASR zero and register-shift selection.
- [x] **ISA-002:** Honor the ALU/shifter flag-write mask. Logical/test/move/shift
  instructions preserve V; arithmetic writes NZCV; instructions whose S bit is clear
  preserve all flags. `tb/unit/alu_tb.sv` checks every logical/test/move and
  arithmetic ALU operation and its exact per-flag mask, while
  `tb/integration/arm7tdmis_flags_preserve_tb.sv` observes the resulting CPSR through
  MRS after an overflowing arithmetic operation, a logical/move S operation, and an
  S-clear arithmetic operation.
- [x] **ISA-003:** Commit N/Z correctly for `UMLALS` and `SMLALS` after the final
  accumulated 64-bit result. Cover all six multiply forms, S/non-S, aliasing, signed
  extrema, and m=1/2/3/4 timing. The final flag commit occurs in `S_MULL_ACC`;
  `tb/integration/arm7tdmis_mull_flags_tb.sv` distinguishes the final accumulated
  value from the premature low-half/product value, and the reset-per-row
  `tb/integration/arm7tdmis_multiply_matrix_tb.sv` executes 21 rows
  covering every form and S setting, zero/nonzero N/Z, C/V preservation, defined
  overlaps, both signed endpoints, and all four m timings. The combinational
  cross-check remains in `tb/unit/multiplier_tb.sv`.
- [x] **ISA-004:** Enforce PSR privilege rules: User mode may change CPSR flags only;
  SPSR access in User/System and writes to invalid modes follow the selected
  UNPREDICTABLE policy; reserved x/s fields are preserved/SBZP; the MSR T-bit policy is
  frozen and tested. `docs/PSR.md` freezes the deterministic policy.
  `tb/unit/psr_policy_tb.sv` seeds nonzero reserved storage and directly proves every
  mask, validity, privilege, RAZ/WI, and restore rule.
  `tb/integration/arm7tdmis_psr_policy_tb.sv` executes the same contract through
  ARM instructions in Supervisor, FIQ, User, and System modes, proves an MSR T-bit
  attempt never changes fetch state, and checks its one-cycle TRM Table 7-6 timing.
- [x] **ISA-005:** Correct every r15 operand value: normal ARM/Thumb reads, ARM
  register-specified shift (+12 case), store-data r15, branch link, PC-relative Thumb
  operations, and debug-state instructions. The reset-per-program
  `tb/integration/arm7tdmis_pc_operands_tb.sv` checks ordinary ARM +8, the distinct
  register-controlled-shift Rm and Rn +12 values, scalar and block-store +12 data, BL link,
  ordinary Thumb +4, and word-aligned Thumb literal/ADD bases at a halfword address.
  The block-store path now uses the latched instruction PC rather than accidentally
  borrowing the following decode-stage PC. `tb/integration/arm7tdmis_cp14_r15_tb.sv`
  and `tb/integration/arm7tdmis_cp_reg_r15_tb.sv` prove MCR r15 +12 and MRC r15
  NZCV-only semantics. Public-scan debug PC accounting, including DBGRQ,
  breakpoint/watchpoint, exception, debug-speed, system-speed, scan-loaded r15, and
  RESTART, remains exact in the DBG-007/JTAG-005 regressions.
- [x] **ISA-006:** Align every PC write according to the destination state. Cover BX
  both directions, data-processing-to-PC, LDR/LDM-to-PC, POP PC, and exception return.
  A CPSR-restoring PC write must use the restored T bit for its first refill.
  `tb/integration/arm7tdmis_pc_write_alignment_tb.sv` is an 11-row reset-per-case
  matrix with deliberately nonzero discarded address bits. It checks the public
  first-target fetch address and SIZE plus the Execute PC/state for ordinary and
  register-shift ARM DP writes, Thumb DP writes, LDR, LDM, POP, both BX directions,
  MOVS-to-PC restoring ARM and Thumb, and LDM^-to-PC restoring Thumb. PC values are
  masked before reaching the raw address pins; restored SPSR.T selects both that mask
  and the first-refill width. Instruction-defined LDR/LDM/POP/BX alignment is
  architectural; deliberately unaligned general DP and exception-return source values
  freeze the ISA-016 masking policy and are not portable software guarantees. The
  register-shift path latches its result and restore
  intent through `S_DP_SHIFT`, eliminating its former next-instruction datapath
  dependency. Existing interworking, LDM return, exception, timing, and debug-PC
  regressions pass unchanged.
- [x] **ISA-007:** Treat Thumb conditional-branch condition `1110` as Undefined, not
  AL. Treat ARM condition `1111` according to ARMv4T (not later unconditional
  encodings). `tb/unit/reserved_decode_tb.sv` exhausts all 4,096 combinations of
  the ARM ARM-defined decode bits `[27:20]`/`[7:4]`, anchors the expected class
  totals (including 445 Undefined rows), repeats all 4,096 rows under
  `cond=1111`, and exhausts all 65,536 Thumb halfwords (5,120 ARMv4T-reserved
  words). Non-decode SBZ/SBO violations that share an allocated decode row are
  architecturally UNPREDICTABLE and remain in ISA-016 rather than being
  mislabeled Undefined. The decoder now rejects signed-store/doubleword,
  unallocated multiply/control/media, and ARMv5 coprocessor-extension holes
  before any execute or external-coprocessor side effect. ARMv4 defines
  `cond=1111` as UNPREDICTABLE; the frozen implementation policy is a precise
  Undefined trap for every lower encoding, never an ARMv5+ unconditional
  instruction. `tb/integration/arm7tdmis_reserved_execute_tb.sv` independently
  executes 12 reset-per-case representatives and checks exact ARM/Thumb LR and
  SPSR values, ARM-state handler entry, successor flush, memory preservation,
  and absence of external `CPnI`.
- [x] **ISA-008:** Implement translated post-indexed loads/stores (`LDRT/STRT`,
  `LDRBT/STRBT`) with User-mode `PROT` while retaining the current processor
  mode. The original “applicable extra-transfer encodings” clause was inaccurate:
  ARM Addressing Mode 3 requires `P=0,W=0`; `P=0,W=1` is UNPREDICTABLE and
  does not encode an ARMv4T translated halfword/signed transfer. The existing
  directed program covers the four immediate-offset mnemonics.
  `tb/integration/arm7tdmis_translated_ls_matrix_tb.sv` adds 16 reset-per-case
  rows covering word/byte, load/store, add/subtract, and immediate/scaled-register
  post-index forms. Every row checks the original base as the transfer address,
  post-index writeback, load/store data and width, User `PROT`, continued
  privileged opcode fetches, and unchanged Supervisor mode.
- [x] **ISA-009:** Freeze exact ARM7TDMI unaligned-access behavior for word,
  halfword, signed-halfword, signed-byte, SWP, and instruction fetches; verify all
  address low bits in both endian modes. ARMv4T `LDR` and the read half of `SWP`
  now rotate the naturally aligned memory word right by
  `8 * effective_address[1:0]`; `STR` and the write half of `SWP` retain the
  aligned-memory rule. Architecturally UNPREDICTABLE odd `LDRH/LDRSH/STRH`
  accesses have an explicitly non-architectural ISA-016 policy matching the
  r4p3 pin behavior: present the calculated address, ignore `ADDR[0]` in the
  memory system, and select the halfword using `ADDR[1]`. The fail-hard
  `tb/integration/arm7tdmis_unaligned_access_matrix_tb.sv` executes 64 data/SWP
  rows (eight access classes x four low-bit values x both endian modes) plus
  eight BX-driven instruction-fetch rows. It checks results, sign extension,
  stores, exact raw address/size/direction, both locked SWP cycles, endian lanes,
  destination state, and word/halfword fetch alignment. The BX `...10` rows are
  explicitly project policy. `arm7tdmis_thumb_halfword_policy_tb` adds 20 Thumb
  reset-per-case rows for register/immediate LDRH/STRH and LDRSH across both
  halfword lanes and endian modes; it found and fixed a Format-8 STRH decoder bug
  that had emitted a byte transfer. `arm7tdmis_thumb_word_unaligned_policy_tb`
  adds 48 register/immediate/SP-relative Thumb LDR/STR rows: 12 aligned controls
  and 36 policy rows across every low-bit value and both endian modes.
  `arm7tdmis_ldr_pc_unaligned_policy_tb` independently checks all eight
  address/endian rows for ARM LDR-to-PC, including six unaligned policy rows.
- [x] **ISA-010:** Verify every load/store P/U/B/W/L combination, immediate and shifted
  register offset, base/destination alias, writeback, r15 use, and defined/
  UNPREDICTABLE case. `tb/integration/arm7tdmis_single_ls_matrix_tb.sv`
  executes all 64 Addressing Mode 2 combinations (P/U/B/W/L x
  immediate/register), including LSL, LSR, ASR, and encoded-RRX shifter paths, and
  checks the exact data-cycle pins, privilege, data, destination, and base
  update. `tb/integration/arm7tdmis_extra_ls_matrix_tb.sv` independently
  executes all 64 Addressing Mode 3 rows (STRH/LDRH/LDRSB/LDRSH x P/U/W x
  immediate/register). Its 16 `P=0,W=1` rows freeze the selected privileged
  post-index result for that architecturally UNPREDICTABLE combination.
  `tb/integration/arm7tdmis_single_ls_policy_tb.sv`
  adds 26 reset-per-case rows for defined aliases and r15 values plus every
  statically detectable unsafe Rn/Rd/Rm/writeback combination class. The selected
  ISA-016 policy takes a precise Undefined trap before any would-be data
  cycle; defined aliases retain their architectural result. Mode 2 `ROR #0`
  now selects RRX in the shared shifter control.
- [x] **ISA-011:** Verify LDM/STM IA/IB/DA/DB, S/W/L, all 16 register-list bits,
  user-bank forms, PC-in-list CPSR restore, base-in-list rules, and a documented empty
  list policy. `tb/integration/arm7tdmis_block_ls_matrix_tb.sv` executes 256
  one-hot rows (P/U/W/L x every r0-r15 list bit) plus 16 eight-register rows
  spanning IA/IB/DA/DB, W, and L; every row checks address order, N/S transfer
  classification, data order, privilege, LOCK, writeback, and r15 behavior.
  `tb/integration/arm7tdmis_block_ls_policy_tb.sv` adds 21 reset-per-case rows
  for privileged User-bank LDM/STM (including STM^ with r15), both LDM^+PC
  writeback forms and CPSR restore, and all base/list policies. STM with the
  writeback base lowest stores the original base; a non-lowest base stores the
  deterministic updated value. LDM with writeback and its base in the list keeps
  the r4p3-compatible Base Updated result. Empty lists, Rn=r15, invalid S/W
  forms, and S forms in User/System take the project-wide precise Undefined
  trap before any data beat. `arm7tdmis_stm_base_list_tb` independently covers
  the defined STM base-lowest rule in all four addressing modes, while
  `arm7tdmis_thumb_block_policy_tb` adds seven Thumb rows for loaded-base-wins,
  base-lowest/non-lowest stores, and every empty-list form.
  `arm7tdmis_ldm_pc_tb`, `arm7tdmis_pc_write_alignment_tb`, and the per-beat
  abort/base-list regressions preserve the neighboring return and abort rules.
- [x] **ISA-012:** Verify SWP/SWPB data, alignment, endian lanes, register aliasing,
  atomicity, and abort behavior. The SWP/SWPB subset of
  `arm7tdmis_unaligned_access_matrix_tb` covers both widths at all four
  address suffixes in both endian configurations, including word-read
  rotation, aligned word-store behavior, byte lanes, data, and both locked
  transfers. `arm7tdmis_swp_policy_tb` adds 18 reset-per-case rows: distinct
  operands and the defined Rd=Rm exchange in Supervisor/User modes, plus
  precise pre-transfer Undefined traps for Rn=Rd, Rn=Rm, and each r15 operand
  position for both widths. `arm7tdmis_swp_bus_matrix_tb` adds 16 pin-level
  rows for normal completion, independent read/write stalls, read/write
  aborts, reset cancellation in either response phase, and DBGRQ between the
  read and write. Every accepted pair is uninterrupted, same-address,
  correctly sized, privileged, and locked; cancellation paths prove memory,
  Rd, handler state, and LOCK release.
- [x] **ISA-013:** Prove condition-failed ARM instructions have exactly the documented
  bus cycle and no register, CPSR/SPSR, memory, lock, coprocessor, or exception side
  effect. Include a condition-failed undefined instruction before SWI and PABT.
  `arm7tdmis_cond_fail_matrix_tb` executes 1,092 reset-per-row cases: Supervisor
  and User modes × all 14 conditions that can fail × 39 paths spanning every
  ARM decode class, multicycle detour, policy-Undefined route, external
  coprocessor handshake, and supported CP14 side effect. Each row proves the
  Table 7-23 incoming PC+8 opcode and outgoing PC+12 address halves are one
  word-read S cycle with correct opcode/data and User/privileged PROT, then compares all
  31 physical GPRs, CPSR, all five SPSRs, all test memory, CP14 state, LOCK,
  DMORE, coprocessor pins, and `DBGINSTRVALID/DBGnEXEC`. The successor reaches
  Execute exactly one cycle later. `arm7tdmis_pabt_pipeline_tb` separately
  proves a condition-failed UDF raises no exception and the immediately
  following SWI or aborted fetch independently selects SWI or PABT, with the
  correct vector, LR, and SPSR rather than a false Undefined exception.
- [x] **ISA-014:** Verify all exception-return idioms (`MOVS/SUBS PC`, LDM with
  `S+PC`) from every exception mode and return to ARM and Thumb.
  `arm7tdmis_exception_return_matrix_tb` executes 60 reset-per-row cases:
  FIQ/IRQ/Supervisor/Abort/Undefined × ARM/Thumb destinations × direct
  `MOVS pc,lr`, register-controlled-shift `MOVS pc`, `SUBS pc,lr,#4`,
  `SUBS pc,lr,#8`, register-controlled-shift `SUBS pc`, and
  `LDMIA sp!,{r4,pc}^`. Every row writes and independently checks the selected
  physical SPSR/LR/SP bank, restores a distinct complete CPSR into User mode,
  sets discarded target-address bits, and verifies the first redirected
  address/SIZE/TRANS/PROT tuple, exact Execute PC/state/opcode, full CPSR,
  unchanged SPSRs/memory/other physical GPRs, successor suppression, and the
  LDM data beats plus source-bank SP writeback. The matrix found and fixed two
  implementation defects: fast data-processing returns now classify the first
  target fetch with restored SPSR.M privilege, and LDM-return writeback retains
  the exception mode so a banked r13/r14 base cannot be written into the
  restored destination bank. The older `arm7tdmis_ldm_pc_tb`,
  `arm7tdmis_pc_write_alignment_tb`, block-transfer matrices, and PSR tests
  remain independent neighboring evidence. The deliberately discarded bits
  verify the selected ISA-016 alignment policy, not an architectural promise for
  otherwise-UNPREDICTABLE return values.
- [x] **ISA-015:** Test instruction sequences, not only isolated opcodes: forwarding/
  dependency pairs, self-modifying stores under the documented memory contract,
  back-to-back PC changes, back-to-back MRC, and mode/bank transitions.
  `arm7tdmis_sequence_dependencies_tb` executes 16 reset-per-case programs covering
  adjacent DP Rn/Rm/Rs and flag consumers; LDR, post-index, MUL, UMULL, LDM, and
  store-data consumers; immediate System/FIQ/Supervisor bank transitions; an
  already-prefetched opcode patched by STR and then explicitly refetched; three
  consecutive PC redirects through branch, DP, load, and ARM/Thumb BX paths; and
  successor suppression. `arm7tdmis_cp_erratum15_tb` independently executes four
  consecutive external MRCs, including opcode1=x1x and early CPA/CPB behavior, with
  distinct responses and writebacks. `arm7tdmis_thumb_bl_boundary_tb` adds IRQ
  preemption and a suffix-fetch PABT between the Thumb BL halves, returns to retry
  the suffix, and enters an orphan suffix with a seeded User LR. The exception rows
  prove the committed architectural User LR survives complete exception flushes;
  the orphan row proves the suffix consumes LR without hidden prefix state. This
  implementation permits exceptions between the halves. A standalone suffix remains
  architecturally UNPREDICTABLE; the frozen project policy is the deterministic
  LR-based result, not an ARM software guarantee. The unified-memory coherence and
  required explicit PC-changing refill contract are published in
  `docs/INTEGRATION.md`.
- [x] **ISA-016:** Give every architecturally UNPREDICTABLE case a stable policy
  (`trap`, deterministic result, or real-r4p3 behavior). Tests must never accidentally
  elevate that chosen behavior into an ARM architectural guarantee.
  `docs/UNPREDICTABLE.md` is the normative, scope-bounded ledger. Static allocated-field
  violations, including every affected SBZ/SBO field, are frozen by
  `unpredictable_decode_tb` (1,066 ARM traps, 448 Thumb traps, 12 deterministic
  rows) and 24 public-pin trap representatives. Runtime evidence includes 24
  no-SPSR S+PC rows, general PC/BX masking, 20 Thumb odd-halfword rows, 36 Thumb
  unaligned-word policy rows, six unaligned LDR-to-PC rows, 16 Mode-3
  `P=0,W=1` rows, seven Thumb block-policy rows, 21 multiply rows, reset-zero
  checks for all 31 GPR slots/five SPSRs, rejected invalid-SPSR restore, both
  orphan Thumb BL halves, and modulo address rollover across 13
  branch/fetch/block rows plus two LDC/STC rows. The ledger also covers PSR
  preservation/rejection, r4p3 MCR-r15, every JTAG IR/chain selector, and every
  reserved EmbeddedICE-RT address, while explicitly excluding later ISA
  features and violations of external contracts.
- [x] **ISA-017:** Exhaustively verify ARM's 31 physical GPRs (30 banked r0-r14
  locations plus the shared PC) and five physical SPSRs across
  User/System/FIQ/IRQ/SVC/Abort/Undefined. `regfile_banking_tb` performs 599
  operations covering every logical r0-r14 view in all seven modes, every
  physical flat-array slot after each normal/debug/force-User write, all three
  read ports, User/System sharing, FIQ r8-r14, every r13/r14 bank, PC reads in
  both states, CLKEN, and reset. `psr_banking_tb` performs 47 operations across
  all five SPSR write/read/restore/exception-save routes, all mode transitions,
  and User/System RAZ/WI inaccessible-bank policy. The ten-row pin-level
  `arm7tdmis_register_banking_matrix_tb` executes both STM^ and LDM^ from every
  SPSR-owning mode, checks all seven address and response phases, every physical
  register slot, memory, mode, and SPSR preservation. The old phrase “every
  privileged mode” was inaccurate: ARMv4T requires S=0 in System mode, so the
  existing User/System S-form rows correctly take the documented precise
  Undefined policy. `docs/REGISTER_BANKING.md` is the complete mapping.

## 31.4 P0 — exceptions, reset, and aborts

- [x] **EXC-001:** Generate the exact saved LR for the source state and exception:
  ARM SWI/UNDEF +4, Thumb SWI/UNDEF +2, PABT/IRQ/FIQ +4, and DABT +8. Cover every
  exception from ARM and Thumb. The reset-isolated
  `arm7tdmis_arm_exception_lr_tb` and `arm7tdmis_thumb_exception_lr_tb`
  matrices cover all six classes in both states and check the selected event,
  source PC, physical LR/SPSR bank, saved CPSR, vector, handler, and suppressed
  side effects.
- [x] **EXC-002:** Implement and prove the simultaneous priority
  Reset > DABT > FIQ > IRQ > PABT > UNDEF > SWI. Lower-priority pending events must
  be retained or discarded exactly as specified, not merely lost in an `if` chain.
  `arm7tdmis_exception_priority_tb` covers FIQ+IRQ, IRQ+PABT, PABT+UNDEF,
  and PABT+SWI at one instruction; the reset matrix collides Reset with live
  DABT+FIQ; the DABT interlock and held-level interrupt tests prove required
  follow-on service, while marker checks prove discarded instruction-associated
  lower-priority events do not leak.
- [x] **EXC-003:** Implement the DABT+FIQ interlock: enter Abort first, then vector to
  FIQ. An abort must not spuriously set CPSR.F merely because FIQ was coincident.
  `arm7tdmis_dabt_fiq_tb` injects a one-enabled-cycle coincident FIQ and proves
  ABT→FIQ→ABT mode order, both links/SPSRs, untouched Abort-vector restart,
  and the original F bit. `arm7tdmis_interrupt_latency_tb` independently
  checks the interlocked 27-cycle worst case at the raw vector address.
- [x] **EXC-004:** Correct LDM abort behavior: complete the instruction, suppress the
  aborting and every later destination write, preserve r15, and perform requested base
  writeback. Test abort on every beat and base-in-list restoration rules.
  `arm7tdmis_ldm_abort_tb` aborts each of four beats including r15;
  `arm7tdmis_ldm_abort_base_list_tb` repeats every beat with Rn in the list.
  Both require all addresses, precise destination suppression, modified-base
  restoration, Abort entry, and successor flushing.
- [x] **EXC-005:** Correct STM abort behavior: requested base writeback completes.
  Test abort on every beat and verify which stores reached memory.
  `arm7tdmis_stm_abort_tb` aborts every beat independently, requires all four
  transfers to be presented, checks exactly which stores committed, and proves
  requested writeback plus precise Abort entry.
- [x] **EXC-006:** Verify single LDR/STR abort behavior for every pre/post-index and
  writeback form: requested base modification still occurs, a load destination is not
  overwritten, and no later side effect leaks from the aborted instruction.
  `arm7tdmis_ls_abort_modes_tb` executes the complete 16-row P/U/W/L word
  matrix, including both post-index encodings, and checks base outcome,
  destination/memory preservation, link, handler state, and successor suppression.
- [x] **EXC-007:** Reconcile SWP's read-only abort requirement with the external bus
  contract. Once a read abort occurs, do not issue/commit the write and do not change
  the destination. `arm7tdmis_swp_read_abort_tb` independently covers SWP and
  SWPB read aborts. `arm7tdmis_swp_bus_matrix_tb` repeats those cases and
  covers write-response aborts: a read abort issues no write address, neither
  abort commits memory or Rd, the successor is flushed, LR_abt is PC+8, and
  LOCK is released.
- [x] **EXC-008:** Prove PABT metadata follows the fetched instruction and disappears
  when that instruction is flushed by a branch, exception, condition path, or debug
  event. `arm7tdmis_pabt_pipeline_tb` covers branch and exception flushes plus
  condition-failed Undefined followers; `arm7tdmis_pabt_debug_flush_tb`
  retains a tagged abort through halt, redirects r15 through the real scan path,
  and proves all metadata is gone before RESTART.
- [x] **EXC-009:** Verify reset while `CLKEN=0`, reset during every multicycle state,
  minimum-low duration, asynchronous assertion/synchronous release contract, initial
  CPSR/mode/banks, architecturally UNKNOWN register values, bus I cycles, and first
  fetch at zero. Document any deterministic FPGA initialization without depending on
  it as architectural behavior. `reset_sync_tb`, `reset_state_policy_tb`,
  `arm7tdmis_reset_clken_tb`, and the 15-row
  `arm7tdmis_reset_multicycle_matrix_tb` cover the complete contract, including
  every non-Execute state and a live Reset+DABT+FIQ collision.
  `docs/UNPREDICTABLE.md` records zero initialization as a project policy,
  explicitly not an architectural software guarantee.
- [x] **EXC-010:** Verify synchronous level-sensitive nIRQ/nFIQ sampling, masking,
  persistence, late arrival, CLKEN stalls, simultaneous requests, and minimum/maximum
  latency including the documented 27-cycle case.
  `arm7tdmis_interrupt_sampling_matrix_tb` has eight reset-isolated rows for
  every listed sampling case; `arm7tdmis_interrupt_latency_tb` measures both
  the four-cycle synchronized minimum and 27-cycle LDM-abort/FIQ maximum.
- [x] **EXC-011:** Scoreboard every exception-entry and return bus cycle from Table
  7-16, including address, T state, mode/PROT, TRANS, and discarded prefetched data.
  `arm7tdmis_exception_bus_matrix_tb` scoreboards all three cycles for six
  exception classes from ARM and Thumb User state (12 rows).
  `arm7tdmis_exception_return_matrix_tb` scoreboards source, address-only,
  target, and following-target phases for 60 mode/state/form rows, including
  register-shift and LDM-PC returns.

## 31.5 P0 — memory interface and cycle accuracy

Cycle conformance means matching pins on every enabled clock, not matching only the
number of cycles spent in an internal FSM state.

- [x] **BUS-001:** `CFGBIGEND` data/fetch mapping is implemented and the endian,
  MiSTer-profile, debug-lane, and ISA-009 matrices verify instruction-halfword
  selection, byte/halfword extraction, sign extension, stores, SWP/SWPB, and every
  byte lane in both configurations. The canonical FPGA wrapper makes endianness a
  synthesis parameter. `arm7tdmis_raw_bus_checker` rejects a change outside reset,
  its mutation matrix proves the assertion, and `docs/RAW_BUS.md` defines the
  raw-integrator violation policy.
- [x] **BUS-002:** Add a phase-aware scoreboard for every clock:
  `ADDR/WRITE/SIZE/PROT/LOCK/TRANS/WDATA/RDATA/ABORT/DMORE`, CP signals, CLKEN, PC,
  CPSR, instruction state, and exception/debug events.
  `table7_core_phase_matrix` now independently scores the full named signal set,
  delayed memory response, architectural identity/state, and events on every phase
  of 17 reset-isolated base-family rows. The reusable raw checker remains bound on
  every clock of every pin-level integration test, and the specialized matrix in
  `docs/TABLE7_MATRIX.md` extends the oracle across redirects, exceptions, aborts,
  stalls, multiplier termination, and coprocessor handshakes.
- [x] **BUS-003:** Re-derive and implement N/S/I/C sequences for every Table 7-2
  category and detailed Table 7-3–7-23 row. Include DP-to-PC, LDR/LDM-to-PC, BX,
  condition fail, exceptions, all multiply m values, and coprocessor busy cases.
  `docs/TABLE7_MATRIX.md` records the direct DDI 0234B derivation, Table 7-2
  counts, all 21 detailed rows, state-machine path, and registered fail-hard
  benches. `verification/constrained_random_rs/src/table7_contract.rs` rejects
  a missing/duplicate table, an unregistered citation, an incomplete central
  signal oracle, or reopened task evidence.
- [x] **BUS-004:** Reconcile Table 7-2's STM `+I` with Table 7-14's visible bus rows in
  a checked design note; do the same for every apparent summary/detail discrepancy.
  `docs/RAW_BUS.md` gives detailed pin rows precedence over summary occupancy tokens
  and records why STM has no additional raw I phase. The Table 7 base, multiply,
  redirect, exception, coprocessor, and condition-fail matrices enforce that rule.
- [x] **BUS-005:** Generate N versus S from actual burst history. Reset/redirect/first
  accesses cannot be labeled S merely because they are fetches.
  The phase-history generator and reusable checker recognize active increments,
  merged I/S, and the specified LDC/STC stream start. `bus_burst`,
  `fetch_sequence`, redirect/exception matrices, and all bound integration tests
  enforce the contract.
- [x] **BUS-006:** Verify pipelined address/data phase alignment for reads and writes.
  A stalled cycle must keep every address-class and write-data signal stable and cause
  exactly one architectural completion when CLKEN resumes.
  The raw memory is an executable one-phase pipeline model. Load/store, block,
  SWP, coprocessor-memory, debug system-speed, DMORE, and MiSTer-wrapper matrices
  check address/data association, held pins, and single completion across stalls.
- [x] **BUS-007:** Sample ABORT only at the specified enabled S/N data or opcode
  phase, ignore it in I/C, and retain the sample across later stalls/phases.
  `abort_inactive` and `abort_clken` prove ignored assertions; the fetch, single,
  block, SWP, coprocessor, priority, and debug abort matrices cover accepted
  response phases and retained causes.
- [x] **BUS-008:** Verify `PROT[0]` code/data and `PROT[1]` User/privileged for every
  fetch, data access, translated access, exception mode, and debug system-speed access.
  The Table 7, translated-transfer, exception-bus, CPnTRANS, SWP, condition-fail,
  and debug system-speed matrices cover those classes, with the raw checker bound
  to every integration top.
- [x] **BUS-009:** Hold `LOCK` across both SWP data transfers and release it on all
  normal, abort, reset, debug, and stall exits.
  `arm7tdmis_swp_bus_matrix_tb` covers every listed exit independently for
  SWP and SWPB, stalls each response phase for four clocks while requiring
  stable pins/state, and proves a mid-pair DBGRQ waits for atomic completion
  before DBGACK. `arm7tdmis_unaligned_access_matrix_tb` additionally requires
  exactly two locked accepted transfers for every endian/address suffix.
- [x] **BUS-010:** Drive `DMORE` as the §1.5.5 address-class prediction for a
  next data access that has a guaranteed sequential follower, not from the
  returning data phase or a loose "block active" flag. Verify
  first/middle/last/single-beat/stalled/aborted LDM and STM.
  `arm7tdmis_dmore_matrix_tb` has eight reset-isolated LDM/STM rows covering
  one- and three-word lists, every DMORE position, first/middle CLKEN stalls,
  and accepted first/middle response aborts. `arm7tdmis_address_wrap_policy_tb`
  independently checks the two-word prediction across 32-bit address rollover
  in ARM and Thumb block-transfer forms.
- [x] **BUS-011:** Verify burst address increments, control stability, byte
  non-bursting, alignment, reset I cycles, and branch/exception/refill waveforms.
  The reusable checker verifies increment/control/byte rules on every integration
  test; `bus_burst`, unaligned, reset, redirect, exception, and refill matrices
  provide directed coverage. Its legal-plus-eleven-mutation self-test proves every
  checker rule and its fatal propagation.
- [x] **BUS-012:** Publish a precise raw-bus integration contract, including when an
  attached memory samples address, data, ABORT, and CLKEN. Provide a protocol checker
  that downstream cores can instantiate.
  `docs/RAW_BUS.md` is the contract and
  `verification/arm7tdmis_raw_bus_checker.sv` is the reusable checker.

## 31.6 P0 — coprocessor and CP14 behavior

- [x] **CP-001:** Remove the fabricated internal CP15 Main ID and its integration
  test. Bare ARM7TDMI/ARM7TDMI-S has no CP15 (ARM DAI 0099C). With `CPA=CPB=1`, p15
  traps Undefined; a system wrapper may attach its own privileged CP15.
  `tb/integration/arm7tdmis_cp15_undef_tb.sv` proves that an unclaimed p15 MRC
  enters Undefined and does not return a fabricated identification value.
- [x] **CP-002:** Decode all coprocessor classes and opcode fields exactly. CP14
  internal operations must match exact `MRC/MCR p14,0,...` encodings; unsupported CP14
  and CP15 operations trap rather than aliasing a broad `CRn` match.
  Decoder class coverage is in `tb/unit/decoder_tb.sv`; exact supported CP14
  encodings and eight representative aliases are checked by
  `tb/integration/arm7tdmis_cp14_decode_tb.sv`, with r15 transfer semantics in
  `tb/integration/arm7tdmis_cp14_r15_tb.sv` and
  `tb/integration/arm7tdmis_cp_reg_r15_tb.sv`.
- [x] **CP-003:** Drive `CPnTRANS=0` in User mode and 1 in privileged modes. It is
  not an inverse code/data signal.
  `tb/integration/arm7tdmis_cpntrans_tb.sv` checks User, System, Supervisor,
  IRQ, and FIQ opcode/data phases.
- [x] **CP-004:** Register CP pipeline-follow signals according to previous/current
  `CPnOPC/CPnMREQ/CPTBIT` rules and CLKEN. Verify ARM fetch, Thumb, stalls, flushes,
  condition fail, and back-to-back CP instructions.
  `tb/integration/arm7tdmis_cp_pipeline_follow_tb.sv` implements an independent
  previous/current-cycle follower and covers every listed case.
- [x] **CP-005:** Implement CPA/CPB absent, accepted-ready, accepted-busy, completion,
  and late-abandonment protocols for CDP/MCR/MRC/LDC/STC. Accepted instructions cannot
  silently become no-ops.
  `tb/integration/arm7tdmis_cp_cdp_protocol_tb.sv`,
  `tb/integration/arm7tdmis_cp_reg_transfer_tb.sv`, and
  `tb/integration/arm7tdmis_cp_ldc_stc_tb.sv` cover all instruction classes and
  absent/ready/busy/completion behavior.
- [x] **CP-006:** Implement C cycles and MRC/MCR data timing; implement variable-length
  LDC/STC transfers, writeback, `CPSEQ`, aborts, and interrupt/DBGRQ abandonment with
  idempotent busy waits.
  Data timing and C cycles are covered by
  `tb/integration/arm7tdmis_cp_reg_transfer_tb.sv`; variable transfer length,
  N/S termination, `CPSEQ`, direction, and writeback by
  `tb/integration/arm7tdmis_cp_ldc_stc_tb.sv`; every-word LDC/STC abort injection
  by `tb/integration/arm7tdmis_cp_ldc_stc_abort_tb.sv`; and late IRQ, FIQ, and
  DBGRQ abandonment by `tb/integration/arm7tdmis_cp_busy_interrupt_tb.sv` and
  `tb/integration/arm7tdmis_cp_busy_dbgrq_tb.sv`.
- [x] **CP-007:** Implement separate DCC RX and TX buffers. Exact behavior:
  c0 control read; c1 data read/write; W/R ownership transitions; no unintended
  round-trip through a shared storage word.
  The public CP14/JTAG round trip is verified by
  `tb/integration/arm7tdmis_cp14_dcc_tb.sv`; independent storage, ownership,
  processor-side CLKEN behavior, and both simultaneous producer/consumer races are
  verified by `tb/unit/dcc_tb.sv`.
- [x] **CP-008:** Return the selected exact-r4p3 DCC version (`0111` under the default
  profile), implement DCC single-access scan-chain-2 behavior, and verify control reads
  through both CP14 and JTAG.
  `tb/integration/arm7tdmis_cp14_dcc_tb.sv` checks the exact `0x70000000` control
  value through both interfaces and checks the data-response address bit with W both
  set and clear.
- [x] **CP-009:** Implement sticky CP14 Abort Status `DbgAbt`, debug-vs-external-abort
  priority, software clear, and exact register encoding.
  Exact c2 decode, sticky set, software clear, set-over-clear priority, and reset are
  covered by `tb/integration/arm7tdmis_cp14_decode_tb.sv` and `tb/unit/dcc_tb.sv`.
  `tb/integration/arm7tdmis_debug_monitor_mode_tb.sv` covers architectural c2 reads
  after monitor-generated Prefetch and Data Aborts and proves that coincident external
  instruction/data aborts win without setting `DbgAbt`.
- [x] **CP-010:** Drive `DBGCOMMRX` high for a full RX buffer and `DBGCOMMTX` high for
  an empty TX buffer, both gated by DBGEN. Verify every host/processor race and reset.
  `tb/unit/dcc_tb.sv` covers reset, CLKEN, and both ownership races;
  `tb/integration/arm7tdmis_cp14_dcc_tb.sv` covers the public pins through both
  directions and gates/restores each pending-data state with DBGEN.
- [x] **CP-011:** Decide and test the release policy for r4p3 errata [14]
  (non-indexed LDC/STC decode) and [15] (sequential MRC timing).
  The frozen release policy is corrected behavior with no defect-emulation
  parameter: unindexed LDC/STC forms execute architecturally, and every
  consecutive MRC executes independently. The exact affected encodings and
  early-CPA/CPB timing are covered by
  `tb/integration/arm7tdmis_cp_erratum14_tb.sv` and
  `tb/integration/arm7tdmis_cp_erratum15_tb.sv`. See
  `docs/COPROCESSOR.md`.

## 31.7 P0/P1 — EmbeddedICE-RT, JTAG, and ETM-facing behavior

- [x] **DBG-001:** Replace the scaffold debug FSM with conformant halt and monitor
  modes. Implement Debug Control bits 5:0, Debug Status bits 4:0, IFEN/INTDIS, Debug
  Abort Status, DBGRQ/DBGBREAK priority, the exact Table 5-1 register map, and all
  DBGEN gating. Follow each pin's specified sampling/synchronization behavior rather
  than applying a blanket two-flop policy. ARM7TDMI-S r4p3 has no Vector Catch
  register: address `0x02` and every other unlisted slot are reserved.
  Same-edge external DBGBREAK sampling, opcode pipeline tagging/restart, and a
  final-beat LDM data watchpoint are covered by
  `tb/integration/arm7tdmis_debug_external_break_tb.sv`. Synchronous external
  DBGRQ sampling, control-bit-1 Run-Test/Idle synchronization, internal-DBGACKI
  status semantics, INTDIS, register width, and RAZ behavior are covered by
  `tb/integration/arm7tdmis_debug_control_sync_tb.sv`. Watchpoint priority over
  simultaneous DBGRQ and watchpoint interaction with Data Abort, IRQ, and FIQ are
  covered by `tb/integration/arm7tdmis_debug_watchpoint_priority_tb.sv`.
  A Prefetch Abort on the breakpointed fetch is proven to discard the
  breakpoint without debug entry by
  `tb/integration/arm7tdmis_debug_breakpoint_pabt_tb.sv`. Coincident
  one-cycle IRQ and FIQ requests are retained through their banked-state
  transition and vector fetch by
  `tb/integration/arm7tdmis_debug_breakpoint_interrupt_tb.sv`; DBGRQ with
  Undefined, bounced-coprocessor, Prefetch Abort, and load/store Data Abort
  is covered by `tb/integration/arm7tdmis_debug_dbgrq_exception_tb.sv`.
  `tb/integration/arm7tdmis_debug_reserved_regs_tb.sv` writes and reads every
  reserved register address through public JTAG, requires deterministic RAZ/WI
  behavior, and proves that address `0x02` cannot create a false vector breakpoint.
  These benches cover every architecturally reachable halt-mode
  breakpoint/watchpoint exception collision. `tb/integration/arm7tdmis_debug_dbgen_sources_tb.sv`
  completes the DBGEN matrix: public JTAG preloads forced DBGACK, INTDIS, monitor
  mode, an instruction breakpoint, and a data watchpoint before DBGEN is lowered;
  held DBGRQ/DBGBREAK/DBGEXT cannot stop either matched access or inhibit the
  selected IRQ/FIQ, and every gated output remains LOW. Together with
  `tb/integration/arm7tdmis_debug_dbgen_gating_tb.sv`, the DCC integration test,
  and the comparator unit test, this covers every documented §5.7/Appendix A
  disable effect.
- [x] **DBG-002:** Feed Debug Status TRANS from the actual `TRANS[1]`, not PROT.
  Align address/control and read/write data before data-dependent watchpoint comparison.
  `tb/integration/arm7tdmis_debug_status_tb.sv` discriminates live `TRANS[1]`
  from privileged `PROT[1]` during both an active stalled transfer and debug halt.
  `tb/unit/ice_watchpoint_tb.sv` poisons the next address/control phase while checking
  the prior transfer's read/write data, and repeats the check across CLKEN stalls.
- [x] **DBG-003:** Implement exact WP0/WP1 value/mask, XNOR, size, read/write,
  opcode/data, privilege, EXTERN, CHAIN latch, RANGE, and ENABLE semantics. Figure
  5-13 contains no T comparator field; TBIT is Debug Status bit 4, not a watchpoint
  qualifier. `DBGRNG` remains independent of ENABLE but is disabled by DBGEN.
  `tb/unit/ice_watchpoint_tb.sv` covers both units, every control field, address/data
  XNOR masks, data-phase alignment, CLKEN, ENABLE independence, DBGEN/control-disable,
  and CHAIN/RANGE coupling. `tb/integration/arm7tdmis_debug_software_breakpoint_tb.sv`
  programs the §5.21.2 sequence through public JTAG and covers ARM plus both Thumb
  bus lanes and adjacent-halfword false positives in both endiannesses. The unit
  regression also implements the TRM's first-256-except-first-32 RANGE recipe and
  sets CHAINOUT before proving that `DBGnTRST` clears it without a clock edge.
- [x] **DBG-004:** Qualify breakpoints with valid, non-flushed instructions.
  Watchpoints enter debug only after the access and all architectural writeback
  complete; cover LDM/STM, aborts, exceptions, and simultaneous DBGRQ.
  Internal opcode execute/restart and flush cancellation are covered by
  `tb/integration/arm7tdmis_debug_breakpoint_execute_tb.sv` and
  `tb/integration/arm7tdmis_debug_breakpoint_flush_tb.sv`. Internal and external
  multibeat LDM completion are covered by
  `tb/integration/arm7tdmis_debug_watchpoint_completion_tb.sv` and
  `tb/integration/arm7tdmis_debug_external_break_tb.sv`.
  `tb/integration/arm7tdmis_debug_watchpoint_priority_tb.sv` covers final-beat
  STM completion and base writeback, external Data Abort precedence, retained
  one-cycle IRQ/FIQ, and simultaneous DBGRQ. Its exception cases require the mode,
  LR, and SPSR transition plus the first vector fetch before DBGACK, while
  proving that neither the vector instruction nor the post-watchpoint
  instruction executes.
- [x] **DBG-005:** Implement monitor-mode restrictions and generated PABT/DABT,
  including `DbgAbt` and external ABORT precedence.
  `tb/integration/arm7tdmis_debug_monitor_mode_tb.sv` programs the mode and
  comparators through public JTAG, covers generated PABT/DABT without halt or DBGACK,
  ignores external DBGBREAK, reads CP14 c2, and covers external instruction/data abort
  priority. `tb/unit/ice_watchpoint_tb.sv` covers Debug Control bit 5, permitted
  `DBGEXT` qualification, and fail-closed data-dependent, RANGE, and CHAIN
  configurations.
- [x] **DBG-006:** Replace the fixed eight-cycle injection window with an explicit
  instruction accepted/retired handshake. Debug-speed and system-speed execution must
  support wait states and every allowed multicycle instruction, including 16-register
  LDM/STM, without fetching or retiring an extra normal instruction. Halt only at a
  legal boundary; do not freeze and repeatedly present an unfinished external bus
  transfer. The explicit handshake is fail-hard covered at debug speed by a stalled
  12-register writeback LDM plus stalled 16-register STMIB/LDMIB transfers in
  `tb/integration/arm7tdmis_debug_inject_handshake_tb.sv` and at
  system speed by a stalled LDR with temporary `DBGACK`, interrupt masking, and
  automatic re-entry checks in `tb/integration/arm7tdmis_debug_system_speed_tb.sv`;
  OpenOCD's MRS/MSR/STR PSR transfer is covered in
  `tb/integration/arm7tdmis_debug_register_scan_tb.sv`. The maximum block
  transfers cover all 16 external beats, architectural r0-r14 effects, r15
  store/load, and restart from the loaded r15 value.
  `tb/integration/arm7tdmis_debug_inject_matrix_tb.sv` completes the §5.16.1
  debug-speed class matrix with all 16 data-processing opcodes, the
  register-controlled-shift extra cycle, every scalar load/store width and
  signedness, SWP/SWPB, and CPSR/SPSR MRS/MSR. Its 38 ordinary injected words
  each require exactly one accept and retire; every memory family is stalled
  before its response edge and the pending normal instruction stays frozen.
  The system-speed test covers the four permitted at-speed classes—single
  load/store and LDM/STM—including stalls and byte/halfword/word accesses.
- [x] **DBG-007:** Implement debug entry/exit PC formulas, temporary DBGACK behavior,
  pending interrupt preservation, interrupt masking during at-speed execution, and
  the DBGRQ/PC-modify errata policy. Scan-loaded r15 now replaces all stale
  fetch/decode state and the OpenOCD-style branch/RESTART sequence resumes at that
  address without spurious system-speed re-entry; this is fail-hard covered by
  `tb/integration/arm7tdmis_debug_pc_resume_tb.sv`. The DBGRQ STM capture includes
  the exact three-word scan-pipeline bias and survives OpenOCD's ARM7TDMI-specific
  correction in `tb/integration/arm7tdmis_debug_pc_capture_tb.sv`; the same
  regression verifies the distinct pre-execute breakpoint formula. The normal
  multibeat watchpoint formula is covered alongside completion ordering in
  `tb/integration/arm7tdmis_debug_watchpoint_completion_tb.sv`. A watchpoint
  collision with a one-cycle IRQ now proves pending-interrupt preservation and
  correct IRQ mode/LR/SPSR/vector entry, and the Data Abort collision proves
  abort-mode entry before debug halt, in
  `tb/integration/arm7tdmis_debug_watchpoint_priority_tb.sv`. That regression
  also scans r15 after independent Data Abort, IRQ, and FIQ collisions and
  requires the three-address exception-entry correction to recover the fetched
  vector. `tb/integration/arm7tdmis_debug_system_speed_tb.sv` compares public
  r15 scans across ordinary debug-speed execution and a stalled at-speed LDR,
  requiring one address per `N` instruction and exactly three per `S`
  instruction. The corrected-default r4p3 erratum [13] policy treats the
  canonical `DBGRQ` input as synchronous, commits every PC modification
  atomically at a legal halt boundary, and retains its destination through
  pipeline refill for the public r15 scan. Asynchronous board/framework
  sources must be synchronized before this interface; no silicon-defect
  compatibility parameter is provided for a condition outside that interface
  contract. `tb/integration/arm7tdmis_debug_pc_modify_dbgrq_tb.sv` covers the
  synchronous boundaries before, during, and after `MOV pc` and during the
  execute, data, and writeback phases of `LDR pc`.
- [x] **DBG-008:** Implement and verify an external Data Abort during a
  debugger-issued system-speed memory instruction (§5.18.5). The aborted
  access must complete with the ordinary architectural abort effects, enter
  Abort mode before returning to debug state, preserve the debug context and
  system-speed re-entry cause, and neither execute a following normal
  instruction nor lose a pending interrupt. Also prove that an unrelated
  external `ABORT` while the processor is halted on an I-cycle is ignored.
  `arm7tdmis_debug_system_speed_abort_tb.sv` drives the complete public
  NOP/0, NOP/1, LDR/0, RESTART sequence and holds ABORT across the pipelined
  response edge. It scans CPSR/SPSR_abt and r4 to require Abort mode before
  DBGACK, exact pre-abort status save, destination suppression, and bit-33
  re-entry. It separately forces ABORT during halted I-cycles and holds an
  IRQ through the aborted access, enabling it only before a real RESTART and
  requiring the IRQ vector/mode afterward. No RTL change was needed; the
  previously untested ordinary abort path already met §5.18.5.
- [x] **DBG-009:** Resolve the contradictory prose in §5.25 about writes to
  Debug Status against its live Figure 5-17/status-source definition. Freeze
  the release behavior as a live read-only, write-ignored resource unless
  stronger authoritative evidence requires a different disposition; document
  the conflict and prove through public JTAG that writing all status bits
  cannot corrupt TBIT, TRANS, IFEN, synchronized DBGRQ, or internal DBGACK.
  `docs/DEBUG.md` records the selected `RO/WI` policy and its source
  hierarchy. `arm7tdmis_ice_rt.sv` deliberately excludes `0x01` from the
  writable register cases and always muxes the five Figure 5-17 sources.
  `arm7tdmis_debug_status_tb.sv` writes all-zero and all-one payloads through
  public chain 2 in both a CLKEN-stalled active transfer and debug halt,
  requiring all five live bits plus the RAZ upper bits after each write.
- [x] **JTAG-001:** Exhaustively verify all 16 TAP states and transitions, async
  `DBGnTRST`, Capture/Shift/Update-IR, fixed `0001` capture, all five public
  instructions, and BYPASS for every other IR encoding. Fail-hard evidence:
  `tb/unit/jtag_tap_tb.sv`.
- [x] **JTAG-002:** Verify IDCODE bit fields and configurability. The r4p3
  macrocell value `0x7F1F0F0F` is the compatibility default; synthesized
  products override the version, part, and manufacturer fields through top-level
  parameters while bit 0 remains fixed HIGH. Policy and ownership are documented
  in `docs/DEBUG.md`; default fields/serial order are fail-hard checked by
  `tb/unit/jtag_tap_tb.sv`, and overrides by
  `tb/unit/jtag_idcode_config_tb.sv`.
- [x] **JTAG-003:** Implement and verify SCAN_N selection, chain 0, 33-bit chain 1,
  38-bit chain 2, reserved chains, the TRM-defined physical cell order, update
  atomicity, and chain-1 bit 33's entry-cause/debug-speed/system-speed meanings.
  Selection, widths, reserved-chain behavior, physical order, update atomicity,
  and one-shot breakpoint/watchpoint entry cause are fail-hard verified by
  `tb/unit/jtag_tap_tb.sv` and
  `tb/integration/arm7tdmis_debug_entry_cause_tb.sv`. The staged bit-33/RESTART
  system-speed sequence, CLKEN stall, temporary `DBGACK`, IRQ masking, automatic
  re-entry, and high re-entry cause are verified by
  `tb/integration/arm7tdmis_debug_system_speed_tb.sv`.
- [x] **JTAG-004:** Gate TMS/TDI/TCKEN/TDO/TDOEN as specified by DBGEN. Implement the
  required TCK synchronization/RTCK convention or publish a proven synchronous-only
  FPGA debug-port wrapper with a different, explicit interface name. DBGEN gating is
  fail-hard verified by `tb/integration/arm7tdmis_debug_dbgen_gating_tb.sv`.
  `rtl/jtag/arm7tdmis_sync_debug_port.sv` is the explicitly named,
  same-`CLK` command/response transport: it emits exactly one `DBGTCKEN` event
  for each accepted step and deliberately does not claim asynchronous JTAG
  compatibility. Backpressure, disable/reset isolation, event accounting, and
  a complete default-IDCODE scan through the real TAP are verified by
  `tb/unit/sync_debug_port_tb.sv`; its timing contract is in `docs/DEBUG.md`.
- [x] **JTAG-005:** Run end-to-end scan scripts that halt, read/write every register
  and memory through legal debug instructions, use a system-speed access with stalls,
  restart, exercise DCC both directions, set break/watchpoints, and enter monitor mode.
  The OpenOCD-compatible debug-speed LDM/STM round trip for r0-r14, including
  external-bus isolation, is covered by
  `tb/integration/arm7tdmis_debug_register_scan_tb.sv`; scan-loaded r15 and the
  branch/RESTART resume path are covered by
  `tb/integration/arm7tdmis_debug_pc_resume_tb.sv`, and DBGRQ r15 capture by
  `tb/integration/arm7tdmis_debug_pc_capture_tb.sv`, which also covers a
  scan-programmed instruction breakpoint; normal data-watchpoint r15 capture is
  covered by `tb/integration/arm7tdmis_debug_watchpoint_completion_tb.sv`.
  OpenOCD-compatible CPSR/SPSR read/write and scan-bus isolation are covered by
  `tb/integration/arm7tdmis_debug_register_scan_tb.sv`. Bidirectional CP14/JTAG DCC,
  including rev-4 single-access status and public status pins, is covered by
  `tb/integration/arm7tdmis_cp14_dcc_tb.sv`. Public-JTAG word, byte, and halfword
  memory round trips plus a stalled system-speed transfer are covered by
  `tb/integration/arm7tdmis_debug_system_speed_tb.sv`; public-JTAG monitor entry and
  breakpoint/watchpoint programming are covered by
  `tb/integration/arm7tdmis_debug_monitor_mode_tb.sv`. A real debugger process is the
  separate `JTAG-006` requirement.
- [x] **JTAG-006:** Demonstrate a pinned open-source debugger/GDB flow against the
  simulated scan transport and on FPGA, or document precisely why the r4p3 scan
  protocol needs a project-specific bridge and release that bridge with protocol tests.
  The release selects the explicit bridge alternative, not a claim that an
  asynchronous pod or GDB process was run. ARM7TDMI-S exposes synchronous
  `DBGTCKEN/DBGTMS/DBGTDI` sampled by `CLK`, not a physical TCK or RTCK
  handshake; `docs/DEBUG.md` therefore forbids direct asynchronous-pod
  attachment without another independently verified CDC bridge.
  `arm7tdmis_sync_debug_port` is the released project-specific bridge: a
  same-CLK ready/valid virtual-TCK step produces exactly one `DBGTCKEN` event,
  buffers TDO/output-enable under backpressure, and fail-closes under reset or
  disabled debug policy. It is part of the public MiSTer wrapper and QIP.
  `sync_debug_port_tb.sv` verifies hold/replacement backpressure, exact event
  count, reset/disable isolation, and a complete IDCODE scan through the real
  TAP. The full register/PSR/memory/DCC/breakpoint/watchpoint/monitor scan
  semantics behind that transport remain the checked JTAG-005 integration
  matrix. `verification/constrained_random_rs/src/debug_bridge_contract.rs`
  locks the selected alternative and prevents it from being mislabeled as
  asynchronous JTAG interoperability.
- [x] **ETM-001:** Define and verify `DBGINSTRVALID` and `DBGnEXEC` for commit,
  condition failure, stalls, multicycle instructions, flushes, exceptions, debug, and
  Thumb. Expose the documented bus/pipeline information required by ETM7.
  `arm7tdmis_etm_trace_matrix_tb` covers every listed runtime class, including
  condition-passing Undefined versus condition failure.
  `arm7tdmis_debug_inject_matrix_tb` requires one executed trace pulse for each of
  38 debug-speed instructions. `docs/TRACE.md` defines the semantics.
- [x] **ETM-002:** Provide and test the external ETM wrapper contract, including direct
  RDATA/WDATA visibility, `PROCID=0`, `PROCIDWR=0`, and reset propagation.
  `arm7tdmis_etm7_adapter` implements the complete Table 6-1 transparent mapping,
  both clock aliases, ETM-to-CPU DBGRQ, reset/TAP-reset propagation, direct data,
  and process-ID tie-offs. `etm7_adapter_tb` verifies every mapping. This is an
  external-macrocell adapter, not a claim that the repository implements ETM7.
- [x] **DFT-001:** For the FPGA profile, remove DFT from all "complete" claims and tie
  it off in a named `no_dft` wrapper. If an ASIC profile remains a goal, create a
  separate scan-insertion specification and prove SE/SI/SO rather than tying SO low.
  The misleading `arm7tdmis_chip` facade has been removed.
  `arm7tdmis_no_dft` explicitly ignores SE/SI and ties SO LOW, carries no
  scan-insertion claim, and is excluded from the FPGA file list/QIP. The packaging
  unit test enforces its name, tie-off, wording, and exclusion. No ASIC DFT profile
  is claimed.

## 31.8 Required r4p3 errata matrix

The release matrix must contain all fifteen entries from
FR002-PRDC-002719 7.0, not only the four that still affect r4p3:

| Erratum | r4p3 status in Arm list | Required project action |
|---|---|---|
| [1] Consecutive breakpoints | Corrected before r4p3 | Regression proving corrected behavior |
| [2] False Undefined exception | Corrected | Regression |
| [3] DBGRQ coincident with exception | Corrected | Regression |
| [4] Breakpoint after exception/watchpointed store | Corrected | Regression |
| [5] Breakpoint after multicycle instruction | Corrected | Regression |
| [6] Watchpoint followed by exception | Corrected | Regression |
| [7] Debug entry coincident with DBGRQ | Corrected | Regression |
| [8] Interrupt during at-speed debug instruction | Corrected in r4p3 | Regression with long LDM/STM |
| [9] wrong ICE version through JTAG | Corrected in r4p3 | Return selected r4p3 value consistently |
| [10] Thumb EIS log error | Corrected / non-synthesized | Not applicable: the repository has no EIS logger/disassembler, and the Arm notice excludes the netlist and ETM; retain independent ETM-facing tests |
| [11] SWI/PABT after condition-failed Undef | Present in r4p3 | Corrected only: a failed UDF retires as one S cycle and the following SWI/PABT is selected independently; no defect-emulation parameter; `arm7tdmis_cond_fail_matrix_tb.sv`, `arm7tdmis_pabt_pipeline_tb.sv` |
| [12] Thumb DABT LR low-bit error | Corrected in r4p3 | Regression |
| [13] async DBGRQ during PC modification | Present in r4p3 | Corrected default: synchronous `DBGRQ` contract, atomic PC commit, retained redirect regression; synchronize asynchronous sources externally |
| [14] non-indexed LDC/STC decode | Present in r4p3 | Corrected only: execute P=0,U=1,W=0 LDC/LDCL/STC/STCL architecturally; no defect-emulation parameter; `arm7tdmis_cp_erratum14_tb.sv` |
| [15] sequential MRC timing | Present in r4p3 | Corrected only: execute each consecutive opcode1=x1x MRC independently, including early CPA/CPB; no defect-emulation parameter; `arm7tdmis_cp_erratum15_tb.sv` |

- [x] **ERR-001:** Record the official errata source and exact hash without
  redistributing its restricted PDF, review every entry's full conditions, freeze the
  policy column, and link each applicable test/result.
  `docs/ERRATA.md` records the official URL, byte length, SHA-256, review date,
  all 15 full-condition dispositions, exact tests, and the reviewed passing baseline.
  `make -C scripts errata` is the reproducible focused evidence target.
- [x] **ERR-002:** Default to architectural corrections. Any real-silicon defect
  emulation is opt-in, has a named parameter, and is included in regression and docs.
  The release has no defect-emulation parameters: [11], [13], [14], and [15], which
  remain present in physical r4p3, are corrected only. Introducing emulation requires
  a new versioned compatibility profile and dual-setting tests. See `docs/ERRATA.md`.

## 31.9 P0 — MiSTer and PocketStation integration

- [x] **MIST-001:** Add a synthesizable `arm7tdmi_mister` wrapper with documented
  request/ready-or-done memory transactions, address, read/write data, byte enables,
  code/data, privilege, lock, sequential/more hints, and abort/error response.
  `rtl/top/arm7tdmi_mister.sv` is the canonical single-outstanding
  valid/ready wrapper and `docs/INTEGRATION.md` is its versioned port/timing
  contract. `make -C scripts lint-mister` elaborates it as a release-facing
  top; `tb/integration/arm7tdmis_mister_wrapper_tb.sv` exercises every
  metadata field and byte-lane class through a real program.
- [x] **MIST-002:** Bridge raw ARM `CLKEN` semantics without gated/generated clocks.
  A request remains stable until completion, slow memory inserts arbitrary waits, and
  each request completes exactly once.
  The wrapper uses only `CLK` and clock enables, has one request slot plus one
  response holding register, and accepts `MEM_READY` independently of
  `CPU_CE`. The integration regression completes the first response while CE
  is low, then randomizes both CE and memory waits while checking full-payload
  stability, exact handshake accounting, and architectural readback.
- [x] **MIST-003:** Define reset and CDC ownership. Synchronize asynchronous board/
  framework signals at the wrapper, keep architecturally synchronous nIRQ/nFIQ clear
  inside the CPU contract, and test reset/interrupt arrival at every phase.
  `docs/INTEGRATION.md` assigns every boundary to `CLK` or an explicitly
  named `_ASYNC` input. The wrapper uses marked two-flop synchronizers for
  active-high IRQ/FIQ and all debug event/policy inputs, then converts only
  the synchronized interrupt levels to raw active-low pins. A parallel,
  preserved two-flop reset synchronizer asynchronously asserts but
  synchronously releases every wrapper/debug reset domain on the same edge as
  the raw core's architectural reset synchronizer.
  `tb/integration/arm7tdmis_mister_cdc_reset_tb.sv` injects IRQ/FIQ during
  normal execution, an unready request, and a response buffered with CE low;
  it deasserts reset between clocks and checks both release stages, then
  asynchronously resets a live request and requires a clean vector-0 restart.
- [x] **MIST-004:** Parameterize little/big endian and optional debug/coprocessor
  features without changing architectural behavior or leaving floating ports. Compile
  and regress every supported parameter combination.
  `BIG_ENDIAN`, `ENABLE_DEBUG`, and `ENABLE_COPROCESSOR` are synthesis
  parameters with deterministic internal tie-offs. One elaboration of
  `tb/integration/arm7tdmis_mister_profiles_tb.sv` instantiates and executes
  all eight combinations. It checks both external byte-lane maps, a complete
  IDCODE scan versus disabled-debug isolation, and claimed versus
  forced-absent external CDP behavior for each relevant profile.
- [x] **MIST-005:** Add complete `.qip`/file-list/QSF integration fragments and an
  example top that can be consumed without private include paths, hierarchical peeks,
  or hand-edited generated files.
  `fpga/arm7tdmi_mister.{f,qip,qsf,sdc}` and
  `fpga/example/arm7tdmi_mister_example_top.sv` form the portable package.
  `verification/constrained_random_rs/src/fpga_package_contract.rs` proves
  exact source-manifest parity, repository-relative paths, public-fragment
  references, and absence of hierarchy-dependent integration;
  `make -C scripts lint-example` elaborates
  the complete package as the selected Cyclone V example top, and
  `make -C scripts quartus-analysis` reads the QSF/QIP and successfully
  analyzes/elaborates it with Quartus 17.0.2 for `5CSEBA6U23I7`. All three
  checks are release-regression phases. The trimmed example also has the
  fail-hard fit/timing characterization recorded under FPGA-003.
  `fpga/arm7tdmis_conformance.{qsf,sdc}` separately preserves and fits every
  raw debug, JTAG, coprocessor, trace, endian, and bus boundary. The real
  framework build is checked separately under MIST-007.
- [x] **MIST-006:** Add a versioned architectural state export/import handshake for
  MiSTer save states. Include all visible registers, banked registers, CPSR/SPSRs,
  pipeline and any in-flight bus/debug state (including a snapshot between Thumb BL
  halfwords), or quiesce to a precisely defined snapshot boundary. Verify restore
  determinism.
  Defining `ARM7TDMIS_SAVE_STATE` adds the six-signal request/ready indexed
  state port without changing the default wrapper API. Schema 1.0 exports all
  30 physical r0-r14 locations, restart PC, CPSR, and five SPSRs in 37 words.
  The wrapper drains a pending response, completes the current architectural
  instruction, cancels only a side-effect-free speculative request, and
  freezes with no live memory transaction; restore flushes microstate and
  refetches the saved first-unexecuted PC. `docs/SAVESTATE.md` freezes the
  handshake, word map, validation rules, debug exclusion, and whole-system
  responsibilities. `arm7tdmis_mister_savestate_tb.sv` requests behind a
  stalled store, reads/mutates/restores every word, proves identical accepted
  request traces and RAM results across two restores, and imports the exact
  Thumb BL prefix/suffix boundary to check target and LR.
- [x] **MIST-007:** Integrate into a real MiSTer framework build targeting the selected
  Cyclone V device. Archive the exact framework commit, build command, timing report,
  resource report, and generated bitstream hash.
  `examples/mister_framework/arm7tdmi_mister_emu.sv` integrates the canonical
  wrapper and generic SoC into the official
  `MiSTer-devel/Template_MiSTer` project pinned at commit
  `69b8a2acc6d84dd313b5abcba6a17155287ed3d8` and tree
  `b8b642eae80716da93754a3e778a5daa3313350b`. A clean
  `make -C scripts mister-framework` invokes
  `quartus_sh --flow compile Template` with Quartus Lite 17.0.2, top
  `sys_top`, device `5CSEBA6U23I7`, exact 12.500 MHz core clock, and pinned
  fitter seed 4. The checked fit uses 14,188 ALMs, 21,415 registers,
  384,488 memory bits, 39 DSP blocks, and three PLLs; four-corner TimeQuest
  reports +0.031 ns minimum setup, +0.060 ns minimum hold, and zero in every
  unconstrained setup/hold category. The 2,665,704-byte `Template.rbf` has
  SHA-256
  `9405e30c869cd9190603f71434e20093ba584a535b5a61792b3a2602f874068a`.
  Schema `arm7tdmis-mister-framework-v1` records the exact source commit,
  source hash, upstream identity, tool, command, timing/resources, reviewed
  upstream constraint diagnostics, and hashes the build log, map/fit/STA/
  assembly reports, summaries, and RBF. Full regression requires this phase;
  the Rust `release-evidence` command revalidates every field and file hash and
  includes them in the commit-addressed evidence archive.
- [ ] **MIST-008:** Create a PocketStation reference system using legally supplied/
  user-provided BIOS and software images. Implement or stub the documented SRAM, flash,
  interrupt, timer, LCD, sound, and I/O behavior sufficiently to boot; check milestone
  PCs, memory side effects, display output, and interrupt activity.
- [ ] **MIST-009:** Run a PocketStation soak test with real software and randomized
  memory waits, reset, interrupts, and save/restore. Compare an observable execution
  trace against an independent emulator/reference where behavior is defined.
- [x] **MIST-010:** Add a second, generic SoC integration example (ROM/RAM/timer/UART)
  so the API is not accidentally PocketStation-specific. Compile it with at least two
  supported open-source FPGA tool/simulator flows where practical.
  `examples/generic_soc/arm7tdmi_generic_soc.sv` is a synthesizable,
  hierarchy-free system using only the canonical wrapper. Its 812-byte
  repository-authored ARMv4T/Thumb-1 ROM runs seven self-check groups covering
  mixed-width memory lanes, conditions/flags/shifts, short/long multiply,
  block transfer/stack/call-return, locked SWP, BX interworking, and timer IRQ
  entry/return. It publishes `RUN!` progress and permanent `PASS`/`FAIL`
  status/signature registers, emits `GIP` or `F` through a backpressured UART,
  and is observed under randomized CPU enables in
  `arm7tdmi_generic_soc_tb.sv`. The Rust `generic-soc-rom-check` command uses the
  checksum-installed Arm GNU 14.3.Rel1 toolchain to prove every assembled
  byte equals the synthesizable ROM (203 words, SHA-256
  `ee9c06bc1720ce9e4f9f57da3e162acdfc0e42df403766f6d98bbeb9249c4900`);
  Verilator lint/simulation, this equivalence check, and the independent Slang
  11.0 frontend are mandatory regression phases. The real MiSTer `emu` renders
  blue/running, green/pass, or red/fail video plus a 32-bit signature barcode
  and LED states without hierarchy or JTAG. `docs/GENERIC_SOC.md` freezes the
  memory map, software groups, result protocol, tool versions, commands, and
  board interpretation, while `verification/constrained_random_rs/src/integration_contracts.rs`
  enforces the complete contract. Physical execution evidence remains
  FPGA-007.
- [x] **MIST-011:** Publish maximum CPU rate, master-clock/CE ratios, latency rules,
  resource budget, required memories/DSPs, reset duration, endian configuration, and
  optional-feature costs.
  `docs/PERFORMANCE.md` publishes every requested clock, latency, resource,
  power, reset, and endian property from checked reports. Four like-for-like
  `arm7tdmi_mister` fits isolate `none`, `debug`, `coprocessor`, and `both`
  settings on the same device and constraints.
  The Rust `quartus-option-characterization` command validates the reports and emits
  `reports/generated/quartus-options.json`; the mandatory regression phase and
  `verification/constrained_random_rs/src/quartus_reports.rs` tests freeze the configurations,
  delta schema, timing-closing Auto Fit policy, documentation, and evidence
  path. The policy preserves the 0.25 ns minimum synchronous-input contract
  across all four option profiles instead of accepting a one-shot placement
  that can miss external-input hold timing.
- [x] **MIST-012:** Test coexistence with DMA/bus arbitration, including SWP LOCK,
  LDM/STM DMORE, stalls, and abort/error responses.
  `arm7tdmis_mister_dma_arbitration_tb` runs a real ARM program against a
  permanently contending synthetic DMA master and shared memory. Independent
  deterministic CPU-CE and memory-ready stalls produce completions while CE
  is low. The arbiter reserves gaps across both three-beat STM/LDM DMORE
  chains and exactly two locked SWP transfers, while still proving DMA
  forward progress outside those regions. An injected LDR `MEM_ERROR` reaches
  the Data Abort handler exactly once; the resumed program preserves the SWP
  read value and writes its final signature.
- [x] **MIST-013:** Supply checked thin adapters from the canonical wrapper interface
  to the selected MiSTer `enable/done` convention and at least one standard FPGA bus
  (Avalon-MM or Wishbone). Keep the CPU contract independent of any one adapter.
  `arm7tdmi_mister_enable_done_adapter` preserves the complete request payload
  and qualifies the selected active-high host completion. The independent
  `arm7tdmi_wishbone_adapter` maps byte address, data, select, error, and lock
  to conservative B4 classic cycles while exposing canonical metadata as
  optional sidebands; it does not falsely extend DMORE into a Wishbone burst.
  Both are stateless, hierarchy-free public-package sources.
  `adapters_tb` verifies idle qualification, multi-cycle backpressure, exact
  payload/metadata, success, read data, and error mapping for both adapters;
  `fpga_package_contract.rs` enforces file-list/QIP inclusion and independence
  from the canonical wrapper implementation.

## 31.10 P0/P1 — verification closure

- [x] **VAL-001:** Add an independent ARMv4T instruction reference model or
  differential co-simulation. It must not reuse this RTL's decoder or expected-value
  functions. Normalize documented implementation-defined/UNPREDICTABLE cases.
  `verification/constrained_random_rs` compiles one repository-authored
  ARM/Thumb program for `-march=armv4t`, runs it independently on QEMU's
  ARM926 model with one guest instruction per translation block, and converts
  the external CPU log into post-instruction state without importing RTL,
  decode packages, or expected-value functions. The shared-subset policy
  excludes ARMv5 and architecturally UNPREDICTABLE inputs and masks only
  QEMU-only PSR extension bits. `arm7tdmis_qemu_diff_tb.sv` compares 77
  consecutive retirements across ARM/Thumb interworking, conditions, shifts,
  multiply, word/byte/halfword/signed memory operations, block transfers,
  SWP, ARM BL, and Thumb BL: PC/state, r0-r14, CPSR, and final memory must
  agree. `make -C scripts integ-qemu_diff` regenerates the reference and
  records tool/source/output hashes under the generated evidence tree; the
  release archiver includes those files. It is first in the integration
  manifest and therefore mandatory in both quick CI and full local regression.
- [x] **VAL-002:** Generate constrained-random ARM and Thumb programs with instruction,
  register, flag, mode, memory, exception, alignment, endian, and dependency coverage.
  Compare retirement state and permitted memory effects for long seeded runs.
  `verification/constrained_random_rs` deterministically generates 32 independent
  256-instruction seeds for the release campaign. Each seed's shared-ARMv4T
  lane boots at address zero on QEMU ARM926 and the RTL, walks all privileged
  register-bank modes, takes ARM and Thumb SVC/undefined exceptions, and
  randomizes legal ARM/Thumb data processing, flags, shifts, multiply,
  conditions, aligned memory, register choices, and producer/consumer
  dependencies. `arm7tdmis_random_diff_tb.sv` compares every post-instruction
  PC/state, active r0-r14 bank, architectural CPSR field, exception cause, and
  five permitted memory words through the public retirement interface. The
  release minimum is 8,192 QEMU-compared retirements.
  QEMU's later-core unaligned and endian policies are deliberately not used as
  ARM7 evidence. A second generated lane uses a repository-authored
  architecture-derived byte/halfword/legacy-word memory model and
  `arm7tdmis_random_policy_tb.sv` to check all word alignments, signed accesses,
  every byte lane, mutations, and final source/output memory in both little-
  and big-endian profiles. Schema `arm7tdmis-constrained-random-v1` records
  exact tool/input/artifact hashes, required bins, per-seed commands, and an
  exact reproducer. Quick CI runs two 64-instruction seeds; the full regression
  and release-evidence validator require the 32 × 256 campaign.
- [x] **VAL-003:** Integrate legally redistributable public ARMv4T suites (for example
  ARM/Thumb instruction exercisers used by mature open FPGA cores) after recording
  license, source commit, expected signature, and any patches. Do not claim the
  proprietary Arm Validation Suite was run unless it actually was.
  `verification/constrained_random_rs/src/public_suite.rs` fetches the MIT-licensed
  `jsmolka/gba-suite` repository at exact commit
  `a7113b67e63f83a9b321696ddd7042ccfad6c881`, verifies its Git tree,
  origin, clean state, license, and source-ROM hashes against
  `verification/public_suites.json`, and runs both upstream ARM and Thumb CPU
  exercisers. Only the manifest-listed GBA header removal and branches over
  explicitly documented ARM3/ARMv4T UNPREDICTABLE-policy cases are permitted;
  every other payload byte must equal upstream and both patched-image hashes
  are frozen. `tb/integration/arm7tdmis_public_suite_tb.sv` supplies the
  minimal memory/vblank environment and observes only raw CPU pins plus the
  public verification retirement interface. The ARM image passes after 8,596
  retirements (8,588 ARM, 8 Thumb), the Thumb image after 6,691 (6,152 ARM,
  539 Thumb), and both produce frozen VRAM signature `0xe8d71fb2`, for 15,287
  total retirements. Full and quick regression fetch/run this phase; schema
  `arm7tdmis-public-suite-v1` records exact inputs, license, patches,
  artifacts, metrics, and reproducers, and release evidence independently
  rejects stale, dirty, altered, weakened, or hash-mismatched results. The
  proprietary ARM Validation Suite was not run and is not claimed.
- [x] **VAL-004:** Replace the current E-state-duration cycle harness with exact
  Chapter 7 waveform tests and cross coverage over class × register/PC × m/n/b ×
  condition × endian × stalls × abort/interrupt.
  `arm7tdmis_table7_core_phase_matrix_tb.sv` replaces duration-only checking
  with a complete pin/state oracle over 17 base instruction-family rows, both
  endian configurations, and continuous versus deterministic 1-to-4-cycle
  CLKEN holds at every Execute phase (68 reset-isolated cases). It checks every
  enabled Chapter 7 output, response, coprocessor follower, debug/trace event,
  and internal phase; Appendix B's first-stopped-cycle allowance; all later
  stopped edges; and exact phase restoration.
  `verification/table7_cross.json` then binds nine required legal-equivalence
  crosses to the existing register/PC, all-`m`, block-`n`, coprocessor-`b/n`,
  condition/mode, endian/alignment, abort, interrupt, and exception matrices.
  `verification/constrained_random_rs/src/table7.rs` accepts them only from passing phases in the
  same clean full regression, verifies every phase log and testbench SHA-256,
  requires the exact row-count PASS marker, and writes
  `table7-cross-report.json`. The checked manifest accounts for 1,903 evidence
  rows against a 1,891-row minimum with all nine crosses covered and no missing
  required bin; non-applicable dimension combinations are explicit rather
  than counted as waivers.
  `verification/constrained_random_rs/src/table7_cross_contract.rs` and
  `verification/constrained_random_rs/src/release_evidence/validation_contracts.rs`
  prevent the phase, manifest, hashes, dimensions, row minima, release
  validation, or checked status from weakening.
- [x] **VAL-005:** Add randomized CLKEN stalls to every instruction class and random
  legal ABORT, IRQ, FIQ, reset, DBGRQ, and coprocessor handshakes at each cycle.
  `arm7tdmis_random_event_matrix_tb.sv` closes all 16 normalized decoder-class
  bins and a CLKEN-stall bin for each class independently in every seed. Its
  per-cycle LFSR inserts one-to-three-cycle holds, randomizes legal opcode/data
  read/data write ABORT response positions, interrupt arrival, reset class and
  duration, DBGRQ arrival, and ready/busy/absent coprocessor timing. ABORT can
  only accompany an active response, `{CPA,CPB}` can only be `00`, `01`, or
  `11`, and each event must produce the expected abort/interrupt/debug/reset/
  coprocessor outcome.
  `verification/constrained_random_rs/src/random_events.rs` runs 32 deterministic nonrepeating seeds,
  requires all 16 class and ten event bins plus at least 256 random decisions
  in every seed, preserves exact logs and reproducers, and emits
  `arm7tdmis-random-events-v1`. Full regression and release evidence require
  the clean same-commit campaign; mutation tests reject reduced masks,
  shortened/duplicate seeds, stale evidence, missing logs, or a weakened
  decision floor.
- [x] **VAL-006:** Add functional coverage for every valid encoding family and every
  specified exceptional/reserved path. Release has zero uncovered required bins; all
  exclusions cite specification text.
  `verification/functional_coverage.json` defines 234 nonduplicated required
  bins across every ARM class/condition/data-processing opcode/shifter/
  multiply/PSR/branch/memory/block/swap/coprocessor family, all 19 Thumb
  formats and their operation subfamilies, and the reserved plus selected
  UNPREDICTABLE-policy paths. `functional_coverage.rs` independently
  enumerates all 4,096 ARM decode rows, all 4,096 `cond=1111` policy rows,
  and all 65,536 Thumb halfwords. The already exhaustive
  `reserved_decode_tb.sv` and `unpredictable_decode_tb.sv` prove the RTL
  agrees over those complete domains (including 1,066 ARM and 448 Thumb
  static-policy rows), while 13 execution matrices close condition,
  reserved-execution, PC/SPSR, memory, block, swap, CP15, and Thumb BL policy
  bins.
  The full-regression aggregate accepts only hashed PASS logs and owning
  sources from the same clean commit, reports zero uncovered bins, and is
  independently revalidated for release. All three exclusions name their
  ARM DDI/ARMv4T source and retain overlapping reserved or project-policy
  behavior as required bins; mutation tests reject a removed group/bin,
  shortened domain, missing phase, uncited exclusion, stale result, or
  nonzero uncovered set.
- [x] **VAL-007:** Add formal properties for no ghost commits, condition suppression,
  mode/bank isolation, PC alignment, flag preservation, bus stability under CLKEN,
  one completion per request, LOCK lifetime, exception priority, abort suppression,
  TAP transitions, and CDC/reset assumptions. Under fair memory/coprocessor responses,
  also prove forward progress and absence of deadlock.
  `verification/formal_requirements.json` is the exact 14-obligation proof
  map. The actual pipeline, register bank, PSR, TAP, reset/debug
  synchronizers, and canonical MiSTer bridge are elaborated by
  `tb/formal/formal.sby`; the bridge proof substitutes only a nondeterministic
  raw-core output model so its ownership rules hold independently of current
  CPU behavior. Safety/alignment/liveness use ABC PDR, and bounded
  bridge/stall/small-module proofs use Boolector k-induction. The only
  liveness assumptions are explicit four-cycle CPU-enable, memory-response,
  and legal coprocessor-response fairness bounds. Formal counterexamples
  exposed and closed CLKEN-dependent coprocessor/Undefined behavior,
  exception-priority masking, and MiSTer write-data ownership under target
  backpressure.
  The Rust `oss-cad-suite-install` command checksum-installs OSS CAD Suite 2026-07-29;
  The Rust `formal-report` command rejects a task without `PASS`, records every source/log
  hash, and emits schema `arm7tdmis-formal-v1`. Formal is mandatory in the
  non-quick regression, scheduled/manual CI, traceability, and release
  evidence; release validation independently rejects dirty, stale, weakened,
  incomplete, or tool-mismatched reports.
- [x] **VAL-008:** Add bounded formal cover traces for every FSM state/transition and
  every exception/debug entry and return.
  The reviewed manifest names all 17 reachable core control states, all 40
  implemented state transitions, 13 exception entry/return obligations, and
  seven debug entry/restart obligations. `core_cover` reaches the core and
  exception set within 80 steps; `debug_cover` reaches the independent
  EmbeddedICE-RT set within 32. Prefetch- and data-abort returns carry a
  formal-only origin bit so synthesis cannot merge their shared Abort-mode
  predicates. The report accepts a cover only when its exact named property
  appears in the SBY `PASS` summary, producing 77/77 independently hashed
  traces and an empty uncovered list.
- [x] **VAL-009:** Add software-level compiler tests built with pinned
  `arm-none-eabi` tools for `-march=armv4t`, in ARM, Thumb, and interworked code.
  The Rust `arm-toolchain-install` command downloads Arm GNU Toolchain 14.3.Rel1 from Arm's
  release endpoint, accepts it only at the recorded SHA-256, and caches a
  versioned install with provenance metadata. The Rust `compiler-program-build` command
  compiles separate `-marm` and `-mthumb` C translation units plus ARM startup
  code with `-mthumb-interwork`, rejects emitted `BLX`, requires ELF
  `Tag_CPU_arch: v4T`, and records commands, tool identity, and source/output
  hashes. `arm7tdmis_compiler_tb.sv` executes that exact image through the raw
  bus, requires ARM and Thumb retirements plus both state-transition
  directions, and fail-hard checks the compiler program's arithmetic, loop,
  stack/call, and word/halfword/byte memory signature. `integ-compiler` is
  mandatory in full regression and, with the QEMU differential, in quick CI;
  generated compiler evidence is included in the release archive.
- [x] **VAL-010:** Run long deterministic fuzz/soak jobs under sanitizing simulator
  settings, X-propagation where supported, and multiple seeds. Archive failing seeds
  and minimize them into directed regressions.
  `verification/constrained_random_rs/src/soak.rs` runs 256 deterministic, nonrepeating seeds through
  a 64-iteration MiSTer-wrapper program while independently randomizing
  `CPU_CE` and memory readiness. The dedicated Verilator build enables unique
  X assignment/initialization and instruments all generated C++ with AddressSanitizer
  and UndefinedBehaviorSanitizer; each child has fail-fast sanitizer settings,
  a per-seed timeout, and an exact seed-specific PASS requirement. Schema
  `arm7tdmis-soak-v1` records the commit and dirty state, simulator identity,
  binary and testbench SHA-256, configuration, and every seed/output result;
  release evidence requires that recorded state to be clean. On failure
  the harness greedily minimizes the 16-bit seed while requiring the same
  timeout, sanitizer class, or normalized testbench diagnostic, preserves the
  original log, and writes an exact command/environment reproducer ready to
  promote into a directed regression. `make -C scripts soak` is a mandatory non-quick
  regression phase and a scheduled/manual CI job whose always-run artifact
  upload retains the report and any failure reproducers.
- [ ] **VAL-011:** Have a reviewer independent of the implementation map each
  sign-off requirement to evidence and inspect waveforms for the highest-risk
  exception, bus, coprocessor, and debug cases.

## 31.11 P0/P1 — synthesis, timing, CDC, and FPGA quality

- [x] **FPGA-001:** Fix `scripts/arm7tdmis.sdc`: there is no `DBGTCK` port; the design
  uses `CLK` plus `DBGTCKEN`. Select the actual synthesis top before constraining DFT
  pins. Do not false-path synchronous nIRQ/nFIQ inputs as if the RTL synchronized them.
  The rewritten constraint file explicitly targets `arm7tdmis_top`, defines
  only `CLK`, treats `DBGTCKEN` and raw interrupt/debug controls as
  synchronous inputs, and contains no chip-wrapper DFT pins.
  `verification/constrained_random_rs/src/fpga_contract.rs` checks those invariants and separately
  checks the canonical FPGA wrapper's real clock/async boundary.
- [x] **FPGA-002:** Choose exact MiSTer/Cyclone V part and board clock. Constrain all
  real clocks, generated enables/interfaces, I/O delays, async controls, reset recovery/
  removal, and legitimate CDC paths. Report zero unconstrained endpoints.
  The selected official MiSTer template targets `5CSEBA6U23I7`; its PLL supplies
  the CPU's exact 12.500 MHz / 80.000 ns `clk_sys`. The integration SDC derives
  the framework PLL clocks, models both runtime-selected HDMI launch/capture
  pairs, the forwarded DDIO clock, audio MCLK and divided I2S clock, and the
  ADV7513 video and audio setup/hold requirements. It enumerates each genuinely
  asynchronous open-drain/status input and each non-clocked board/status output
  instead of using a wildcard waiver. The clean Quartus Lite 17.0.2 four-corner
  fit at fitter seed 4 reports +0.031 ns minimum setup and +0.060 ns minimum
  hold slack, with zero unconstrained clocks, input ports, input-port paths,
  output ports, and output-port paths. The Rust `mister-framework` runner in
  `verification/constrained_random_rs/src/mister_framework.rs`
  rejects a wrong core clock hierarchy/frequency, wrong seed, negative slack,
  critical warning, unexpected ignored constraint, any nonzero unconstrained
  category, or missing bitstream; its mutation tests are mandatory in
  `make -C scripts harness-unit`. `docs/AC_TIMING.md` separates these real-board
  constraints from the TRM's provisional synthesized-macrocell percentages.
- [x] **FPGA-003:** Run clean Quartus analysis/synthesis, fit, TimeQuest at all required
  corners, and assembly for both conformance and trimmed MiSTer profiles. Treat critical
  warnings as failures.
  `make -C scripts quartus-compile` closes the trimmed
  `arm7tdmi_mister_example_top` profile, and
  `make -C scripts quartus-conformance-compile` closes the raw,
  feature-complete `arm7tdmis_top` profile in Quartus Lite 17.0.2. In both
  profiles synthesis, fit, assembly, and four-corner TimeQuest complete; all
  setup/hold/pulse-width slacks are nonnegative; the design is fully
  constrained; and a nonempty SOF is emitted.
  The Rust `quartus-report-check` command rejects missing stages/images, critical
  warnings, ignored constraints, unconstrained endpoints, negative slack, wrong
  top/device, and profile-specific resource-budget overruns. Both full flows
  are mandatory phases of `make -C scripts regress`; quick regression runs
  analysis/elaboration for both.
- [x] **FPGA-004:** Run an independent synthesizer/linter where supported and CDC/RDC
  analysis. Resolve combinational loops, inferred latches, multiple drivers,
  simulation/synthesis mismatches, unsafe synchronizers, and reset-domain crossings.
  The Rust `independent-lint` command requires checksum-installed Slang
  11.0.0+7ddf4059f and independently elaborates the raw core, canonical
  wrapper, no-DFT compatibility top, FPGA example, and generic SoC with zero
  errors and zero warnings. It records tool/source/diagnostic hashes in
  schema `arm7tdmis-independent-lint-v1`; Slang's elaboration and data-flow
  analysis independently reject latches, loops, driver conflicts, and
  simulation/synthesis-invalid constructs already forbidden by fatal
  Verilator and Quartus gates.
  The Rust `cdc-rdc` command checks every production `always_ff` against the
  reviewed `verification/cdc_rdc_manifest.json`: all 37 sequential blocks
  use the sole `CLK` domain, all 15 asynchronous reset sensitivities use an
  approved raw or synchronized-release reset, both reset-release instances
  are structurally present, and each of six async event inputs has exactly
  first-stage → second-stage fanout with both flops marked. Mutation unit
  tests prove extra clocks, generated resets, direct async control use, and
  first-stage fanout fail. The audit found and removed direct asynchronous
  release of wrapper/debug state by adding the parallel preserved
  `wrapper_reset_n` synchronizer and an off-edge release regression.
  `make -C scripts fpga-quality` runs both gates; they are mandatory in every
  regression and their clean, same-commit, input-hashed reports are required
  release evidence.
- [x] **FPGA-005:** Verify RAM/DSP/clock-enable inference in reports. Publish ALM,
  register, MLAB/M10K, DSP, clock, power estimate, and Fmax data with budget headroom.
  Both complete flows now run vectorless PowerPlay before
  the Rust `quartus-report-check` command, which fails on absent Fmax, power,
  confidence, or clock-enable fields in addition to the existing fit/timing
  gates. `docs/PERFORMANCE.md` publishes the checked resources, clock-enable
  inference, zero MLAB/M10K bits, six DSP blocks, budget headroom, worst-model
  Fmax, and explicitly low-confidence power estimates.
  `verification/fpga_characterization.json` binds every published profile to
  the producing clean commit, exact characterization commands, and SHA-256 of
  every RTL, project, constraint, top, and checker input.
  `characterization_contract.rs` and the Rust `quartus_reports.rs` tests
  freeze the flow, schema, input hashes, and cross-document metrics so stale
  characterization cannot remain checked.
- [x] **FPGA-006:** Prove synthesis equivalence or run post-synthesis simulation for
  architectural smoke, stalls, reset, endian, exceptions, and wrapper transactions.
  The Rust `verification/constrained_random_rs/src/postfit_sim.rs` runner
  creates isolated Quartus Lite 17.0.2 projects for
  little- and big-endian `arm7tdmi_mister` instances on `5CSEBA6U23I7`, runs
  synthesis and fit, emits functional post-fit Verilog without modifying any
  checked QSF, rejects unmodeled/encrypted primitives, and executes each
  structural netlist with Verilator. The simulation profile disables automatic
  DSP recognition solely to use the five repository-authored clean-room
  zero-delay primitive models in
  `verification/intel_cyclonev_postfit_primitives.sv`; FPGA-003/005 separately
  require the production DSP, timing, and programming-image flow.
  `arm7tdmi_postfit_tb.sv` checks reset during a held request, response
  buffering while `CPU_CE` is low, randomized independent CPU/memory stalls,
  stable wrapper payloads, reset and IRQ vectors, handler and architectural
  signatures, code/data metadata, all write widths, and exact endian lanes
  through public fitted pins only. `make -C scripts postfit-sim` writes schema
  `arm7tdmis-postfit-v1` with both netlist/primitive/input/tool/log hashes and
  transaction metrics. It is mandatory in full regression; release evidence
  requires a clean same-commit result for both endian profiles.
- [ ] **FPGA-007:** Perform on-board bring-up with an embedded trace/signature test,
  repeatable programming instructions, and captured evidence; then run the
  PocketStation reference integration on hardware.
- [ ] **FPGA-008:** Pin every tool/container version and make the release build
  reproducible from a clean checkout in CI.
- [x] **FPGA-009:** Give every Chapter 8/Table 8-1 AC parameter an explicit
  soft-core FPGA disposition. Preserve the original percentages and
  provisional synthesized-macrocell/silicon scope in a revision-locked
  `verification/ac_timing.json` contract; map applicable
  synchronous input/output budgets into the raw conformance profile's
  target-specific SDC/STA checks, and identify reset, debug synchronization,
  test-only, power, and clock parameters that are replaced or owned by the
  containing FPGA integration. Add a fail-hard constraint/documentation test
  so neither a Table 8 row nor its physical-scope caveat can disappear.
  The contract locks the exact 242-page PDF identity, all 37 Table 8-1 rows,
  all five Chapter 8 figures, the Figure 8-4 naming conflict, Appendix B.3,
  and explicit clock/reset/debug/DFT/power ownership. The Rust
  `verification/constrained_random_rs/src/ac_timing.rs` checker uses an
  independent reference table, validates every source percentage and signal
  group, and requires an exact active SDC marker/command for all
  37 rows; its Rust tests mutate row order, values, scope,
  figure membership, ownership, and constraints to prove fail-closed
  behavior. The raw `5CSEBA6U23I7` profile translates the applicable source
  fractions at 16 MHz without adding an architectural cycle and passes
  four-corner fitted TimeQuest with minimum 1.904 ns setup and 0.161 ns hold
  slack. `docs/AC_TIMING.md` publishes the complete row-by-row disposition;
  the `ac-timing` phase is mandatory in every regression and its clean
  same-commit report is revalidated and archived by release evidence.

## 31.12 Documentation and release hygiene

- [x] **DOC-001:** Replace all unconditional "Complete", "all green", and
  "cycle-accurate" claims in README, AGENTS, and `docs/` with the §31 status vocabulary
  until linked sign-off evidence exists. `README.md` now defines VERIFIED,
  IMPLEMENTED-UNVERIFIED, and PARTIAL; reports subsystem evidence separately from
  whole-release status; and links every summary to the canonical ledger/contracts.
  `verification/constrained_random_rs/src/documentation_contract.rs` rejects the known stale baseline claims.
- [x] **DOC-002:** Correct documentation for LDM/STM aborts, exception priority/LR,
  logical V preservation, PC values, STM/coprocessor cycles, endianness, CP14, absence
  of internal CP15, monitor/debug limitations, ETM, and DFT. The executable
  documentation contract requires each fact in `EXCEPTIONS.md`, `PSR.md`,
  `PIPELINE.md`, `COPROCESSOR.md`, `RAW_BUS.md`, `DEBUG.md`, `TRACE.md`, and
  `INTEGRATION.md`.
- [x] **DOC-003:** Create a bidirectional traceability table:
  requirement ID → source section → RTL → directed/random/formal tests → coverage bin
  → latest result. Also prove every RTL feature and every test maps back to a
  requirement.
  `verification/traceability.json` is the reviewed ownership map and
  `verification/constrained_random_rs/src/traceability_report.rs` expands the
  canonical §31 ledger into
  schema `arm7tdmis-traceability-v1`: all 122 requirement rows carry source,
  RTL/disposition, tests/disposition, acceptance bin, ledger status, and the
  latest available regression result. Its reverse tables map every tracked RTL
  file and every tracked SystemVerilog/Rust/program verification source back
  to owning requirement IDs. `make -C scripts traceability` fails on any
  empty row, unknown ID, or unmapped source. It is the final mandatory
  regression phase; the Rust `release-evidence` command verifies its schema, commit, and
  empty-unmapped lists before archiving it. `docs/TRACEABILITY.md` defines the
  bidirectional scope and result semantics, and
  Rust tests in `traceability_report.rs` prevent any side of the contract from
  silently shrinking.
- [x] **DOC-004:** Publish the two integration APIs with timing diagrams, reset and
  clock rules, parameter/tie-off tables, error behavior, synthesis examples, and
  compatibility/version policy.
  `docs/INTEGRATION.md` and `docs/RAW_BUS.md` are the version-1 public
  contracts and contain the edge diagrams, reset/clock/response rules,
  option or tie-off tables, error semantics, complete example ownership,
  checked synthesis commands, and compatibility policy.
  `verification/constrained_random_rs/src/release_evidence/release_hygiene_contracts.rs` fails if either contract loses a
  required section.
- [x] **DOC-005:** Record copyright/license/SPDX status for RTL, tests, manuals,
  third-party suites, reference emulator code, MiSTer fragments, firmware, and ROM
  handling. Do not redistribute copyrighted BIOS/software without permission.
  `docs/PROVENANCE.md` inventories the repository-authored RTL, tests,
  hand-encoded programs, tools, docs, FPGA fragments, included Arm manual,
  non-vendored errata, absent third-party suites/reference emulators, and
  user-supplied firmware/ROM policy with explicit SPDX conclusions.
  `verification/constrained_random_rs/src/release_evidence/release_hygiene_contracts.rs` enforces the document and license
  vocabulary.
- [x] **DOC-006:** Add a changelog, semantic version, support matrix, known-limitations
  file, security/debug notes, and a release manifest containing source/tool/spec hashes.
  `VERSION`, `CHANGELOG.md`, `docs/SUPPORT.md`, `docs/LIMITATIONS.md`, and
  `SECURITY.md` publish the prerelease contract without weakening any open
  gate. The Rust `release-evidence` command writes `release-manifest.json` with the
  project version; commit, Git-tree, and source hashes; canonical tool-version
  and executable manifests; exact TRM/license hashes; and every evidence-file
  hash. `verification/constrained_random_rs/src/release_evidence/release_hygiene_contracts.rs` locks the schema and documents.
- [x] **DOC-007:** Remove stale "pending", "scaffold", and "deferred" comments only
  when the corresponding requirement is verified; otherwise keep them and link the
  owning task ID. Obsolete scaffold/milestone language was removed from the verified
  memory and JTAG paths; remaining occurrences describe real pending state or
  deliberate same-instruction deferral. `verification/constrained_random_rs/src/documentation_contract.rs` prevents
  those two stale comments from returning.
- [x] **DOC-008:** Add a revision-locked, executable coverage inventory for
  the complete ARM DDI 0234B r4p3 manual: all 207 numbered sections and
  subsections, 65 numbered tables, 44 numbered figures/timing diagrams, and
  every Appendix A signal. Record the PDF identity and give every inventory item an explicit
  implementation/test, integration obligation, erratum correction, or
  external/out-of-scope disposition tied back to §31. Validation must fail on
  a changed PDF, missing or duplicate inventory item, unknown requirement,
  missing evidence path, or unchecked implementation claim. Make the audit a
  mandatory regression and release-evidence phase so §31 cannot again be
  inferred complete from broad chapter-level prose.
  `verification/trm_coverage.json` records the exact 361-item inventory and
  nineteen nonoverlapping disposition groups. The Rust
  `verification/constrained_random_rs/src/trm_coverage.rs` checker
  independently reconstructs the expected numbering/signal sets, checks the
  PDF bytes and SHA-256, validates every requirement state/evidence path, and
  writes schema `arm7tdmis-trm-coverage-v1`.
  Its Rust contract tests mutate missing/duplicate items, source
  identity, requirement ownership, and evidence paths. `trm-coverage` runs in
  every regression profile, and release packaging revalidates its clean
  same-commit input hashes. `docs/TRM_COVERAGE.md` defines the disposition
  meanings and previously preserved DBG-008/009 and FPGA-009 as explicit
  open rows until their dedicated evidence closed them.

## 31.13 Final v1.0 release gate

The release owner may sign off only when all statements below are true:

```text
[ ] Every P0 item in §31 is checked with a durable evidence link
[ ] Every required §0–§30 item maps to §31 evidence or an explicit OUT-OF-SCOPE decision
[ ] No required area is MISSING, INCORRECT, PARTIAL, or IMPLEMENTED-UNVERIFIED
[ ] Clean reproducible regression fails hard and passes all variants
[ ] ARM/Thumb encoding, functional, exception, bus-cycle, endian, and debug coverage closed
[ ] Independent differential tests, formal properties, compiler tests, and long soaks pass
[ ] All selected r4p3 errata policies are documented and tested
[ ] Raw ARM7TDMI-S and MiSTer wrapper protocol checkers pass with randomized stalls/errors
[ ] Quartus synthesis, fit, timing, CDC/RDC, and post-synthesis checks pass without open waivers
[ ] A real MiSTer build and on-board PocketStation reference run pass
[ ] Save-state restore is deterministic in the PocketStation integration
[ ] Generic second-SoC integration builds and runs
[ ] README/AGENTS/docs exactly match the audited state and link the release evidence
[ ] License/provenance review is complete
[ ] Two reviewers, including one not responsible for the RTL, approve the traceability matrix
[ ] Release tag, source hash, spec hashes, toolchain, artifacts, and reports are frozen
```

P1/P2 work may be deferred only if it is outside both required profiles and appears in
the signed limitations/support matrix. There are no silent waivers. A post-sign-off bug
creates an erratum and a versioned maintenance task; it does not rewrite the historical
v1.0 evidence.
