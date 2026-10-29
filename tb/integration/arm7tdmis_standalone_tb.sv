// Independent program execution through the public VER-009 observation ports.
// No golden trace, QEMU result, or expected exception list is consumed.
`timescale 1ns/1ps

`ifndef ARM7TDMIS_VERIFICATION
    `error "standalone requires ARM7TDMIS_VERIFICATION"
`endif

module arm7tdmis_standalone_tb
    import arm7tdmis_bus_pkg::*, arm7tdmis_types_pkg::*;
#(parameter int MEMORY_WORDS = 65536);

    localparam int MEMORY_BYTES = MEMORY_WORDS * 4;
    // Independent external RAM for GBA-compatible classic Thumb pairs. The
    // CPU sees the actual high bus address; this storage never aliases RAM.
    localparam logic [31:0] IWRAM_BASE = 32'h03000000;
    localparam int IWRAM_BYTES = 32768;
    logic CLK;
    initial begin
        CLK = 1'b0;
        forever #5 CLK = ~CLK;
    end
    logic nRESET = 1'b0;
    logic [31:0] ADDR, WDATA, RDATA;
    logic WRITE, LOCK, ABORT;
    logic [1:0] SIZE, PROT, TRANS;
    logic CPnMREQ, CPSEQ, CPnTRANS, CPnOPC, CPTBIT, CPnI;
    logic DBGACK, DBGnEXEC, DBGINSTRVALID;
    logic [1:0] DBGRNG;
    logic DBGCOMMTX, DBGCOMMRX, DBGTDO, DBGnTDOEN, DMORE;
    logic VER_RETIRE_VALID, VER_RETIRE_THUMB, VER_RETIRE_CONDITION_PASS;
    logic VER_RETIRE_INJECTED, VER_RETIRE_EXCEPTION_VALID;
    logic [31:0] VER_RETIRE_PC, VER_RETIRE_OPCODE, VER_RETIRE_CPSR;
    logic [2:0] VER_RETIRE_EXCEPTION;
    logic [991:0] VER_RETIRE_GPRS;
    logic [159:0] VER_RETIRE_SPSRS;

    arm7tdmis_top u_dut (
        .CLK, .CLKEN(1'b1), .nRESET, .CFGBIGEND(1'b0),
        .nIRQ(1'b1), .nFIQ(1'b1), .ABORT,
        .ADDR, .WRITE, .SIZE, .PROT, .LOCK, .TRANS, .WDATA, .RDATA,
        .CPnMREQ, .CPSEQ, .CPnTRANS, .CPnOPC, .CPTBIT, .CPnI,
        .CPA(1'b1), .CPB(1'b1),
        .DBGEN(1'b0), .DBGRQ(1'b0), .DBGBREAK(1'b0),
        .DBGACK, .DBGnEXEC, .DBGINSTRVALID, .DBGEXT(2'b00),
        .DBGRNG, .DBGCOMMTX, .DBGCOMMRX,
        .DBGTCKEN(1'b0), .DBGTMS(1'b0), .DBGTDI(1'b0),
        .DBGTDO, .DBGnTRST(1'b1), .DBGnTDOEN, .DMORE,
        .VER_RETIRE_VALID, .VER_RETIRE_PC, .VER_RETIRE_OPCODE,
        .VER_RETIRE_THUMB, .VER_RETIRE_CONDITION_PASS,
        .VER_RETIRE_INJECTED, .VER_RETIRE_EXCEPTION_VALID,
        .VER_RETIRE_EXCEPTION, .VER_RETIRE_GPRS, .VER_RETIRE_CPSR,
        .VER_RETIRE_SPSRS
    );

    // The shared memory indexes by low address bits. Abort an unmapped
    // transaction so an invalid access cannot silently alias into RAM.
    logic [31:0] response_address;
    logic response_write;
    logic [1:0] response_size, response_trans;
    always_ff @(posedge CLK) begin
        if (!nRESET) begin
            response_address <= 32'b0;
            response_write <= 1'b0;
            response_size <= 2'(SIZE_WORD);
            response_trans <= 2'(TRANS_I);
        end else begin
            response_address <= ADDR;
            response_write <= WRITE;
            response_size <= SIZE;
            response_trans <= TRANS;
        end
    end
    wire iwram_address = ADDR >= IWRAM_BASE && ADDR < IWRAM_BASE + IWRAM_BYTES;
    wire iwram_response = response_address >= IWRAM_BASE
        && response_address < IWRAM_BASE + IWRAM_BYTES;
    wire [31:0] low_rdata, iwram_rdata;
    wire low_abort, iwram_abort;
    arm7tdmis_memory #(.WORDS(MEMORY_WORDS)) u_mem (
        .CLK, .CLKEN(1'b1), .nRESET, .CFGBIGEND(1'b0),
        .ADDR, .WRITE, .SIZE, .PROT, .LOCK,
        .TRANS(iwram_address ? 2'(TRANS_I) : TRANS), .WDATA, .RDATA(low_rdata),
        .ABORT(low_abort), .inject_abort(response_address >= 32'(MEMORY_BYTES))
    );
    arm7tdmis_memory #(.WORDS(IWRAM_BYTES / 4)) u_iwram (
        .CLK, .CLKEN(1'b1), .nRESET, .CFGBIGEND(1'b0),
        .ADDR(ADDR - IWRAM_BASE), .WRITE, .SIZE, .PROT, .LOCK,
        .TRANS(iwram_address ? TRANS : 2'(TRANS_I)), .WDATA, .RDATA(iwram_rdata),
        .ABORT(iwram_abort), .inject_abort(1'b0)
    );
    assign RDATA = iwram_response ? iwram_rdata : low_rdata;
    assign ABORT = iwram_response ? iwram_abort : low_abort;

    string program_hex, iwram_hex, result_jsonl;
    int result_fd;
    logic [31:0] stop_pc, memory_base;
    int signed memory_length = 128;
    longint signed max_cycles = 1000000;
    logic [31:0] assertion_result_address = 32'b0;
    bit assertion_result_enabled = 1'b0;
    longint unsigned cycles = 0;
    longint unsigned retired_count = 0;
    longint unsigned exception_count = 0;
    logic [31:0] last_retired_pc = 0;
    bit last_retired_thumb = 1'b0;
    bit finished = 1'b0;
    localparam logic [31:0] GUEST_CONSOLE = 32'h0000ff00;
    logic [31:0] guest_frame [0:7];
    int guest_word_count = 0;
    string checkpoint_file;
    // Each observer-only row holds {instruction width, final-ELF start PC}.
    logic [63:0] checkpoint_table [0:65535];
    bit checkpoint_done [0:65535];
    bit checkpoint_half_valid [0:65535];
    logic [15:0] checkpoint_first_half [0:65535];
    int checkpoint_count = 0;
    int checkpoint_observed = 0;
    bit commit_flush;
    logic [31:0] commit_flush_pc;
    logic [31:0] checkpoint_next_pc;

    function automatic int active_index(input int number, input logic [4:0] mode);
        if (number <= 7) return number;
        if (number <= 12) return mode == 5'(MODE_FIQ) ? number + 8 : number;
        case (mode)
            5'(MODE_FIQ): return number + 8;
            5'(MODE_IRQ): return number + 10;
            5'(MODE_SUPERVISOR): return number + 12;
            5'(MODE_ABORT): return number + 14;
            5'(MODE_UNDEFINED): return number + 16;
            default: return number;
        endcase
    endfunction

    function automatic bit sequential_completion_nop;
        logic [31:0] opcode;
        logic [3:0] destination;
        opcode = VER_RETIRE_OPCODE;
        if (VER_RETIRE_THUMB) return opcode[15:0] === 16'h46c0;
        destination = opcode[15:12];
        return opcode[31:28] == 4'he
            && opcode[27:25] == 3'b000
            && opcode[24:21] == 4'b1101
            && opcode[20] == 1'b0
            && opcode[19:16] == 4'b0000
            && opcode[11:4] == 8'b0
            && opcode[3:0] == destination
            && destination != 4'hf;
    endfunction

    function automatic string cause_name(input logic [2:0] code);
        case (code)
            3'(EXC_RESET): return "reset";
            3'(EXC_UNDEF): return "undefined";
            3'(EXC_SWI): return "svc";
            3'(EXC_PREFETCH_ABORT): return "prefetch_abort";
            3'(EXC_DATA_ABORT): return "data_abort";
            3'(EXC_IRQ): return "irq";
            3'(EXC_FIQ): return "fiq";
            default: return "unknown";
        endcase
    endfunction

    task automatic flush_result;
        int error_code;
        string error_text;
        $fflush(result_fd);
        error_code = $ferror(result_fd, error_text);
        if (error_code != 0)
            $fatal(1, "[standalone] result write failed: %s", error_text);
    endtask

    task automatic emit_exception;
        bit faulting;
        bit thumb;
        int width;
        logic [2:0] reason;
        // This frame was emitted by eight completed guest stores. No exception
        // pulse or internal CPU register supplies the event identity.
        if (guest_frame[0] != 32'h41584531 || guest_frame[7] != 32'hbea7bace
            || guest_frame[6] > 1)
            $fatal(1, "[standalone] malformed guest exception console frame");
        case (guest_frame[1])
            4: reason = 3'(EXC_UNDEF);
            8: reason = 3'(EXC_SWI);
            12: reason = 3'(EXC_PREFETCH_ABORT);
            16: reason = 3'(EXC_DATA_ABORT);
            24: reason = 3'(EXC_IRQ);
            28: reason = 3'(EXC_FIQ);
            default: $fatal(1, "[standalone] unknown guest exception vector");
        endcase
        faulting = reason != 3'(EXC_IRQ) && reason != 3'(EXC_FIQ);
        thumb = guest_frame[4][5];
        width = thumb ? 2 : 4;
        $fwrite(result_fd,
            "{\"kind\":\"exception\",\"backend\":\"arm7tdmi-sv\",\"provenance\":\"guest_asm_console\",\"sequence\":%0d,\"cycle\":%0d,\"raw_reason_code\":%0d,\"cause\":\"%s\",\"saved_status\":\"0x%08x\",\"resume_pc\":\"0x%08x\",\"faulting_instruction\":%s,\"pc\":",
            exception_count, cycles, reason, cause_name(reason),
            guest_frame[4], guest_frame[5], faulting ? "true" : "false");
        if (faulting) $fwrite(result_fd, "\"0x%08x\"", guest_frame[2]);
        else $fwrite(result_fd, "null");
        // A failed fetch has a fault PC but no successfully fetched opcode.
        if (faulting && guest_frame[6] == 1) begin
            $fwrite(result_fd,
                ",\"thumb\":%s,\"instruction_width\":%0d,\"instruction\":\"0x%08x\",\"instruction_bytes\":\"",
                thumb ? "true" : "false", width, guest_frame[3]);
            for (int b = 0; b < width; b++)
                $fwrite(result_fd, "%02x", guest_frame[3][b * 8 +: 8]);
            $fwrite(result_fd, "\"");
        end else begin
            $fwrite(result_fd,
                ",\"thumb\":null,\"instruction_width\":null,\"instruction\":null,\"instruction_bytes\":null");
        end
        $fwrite(result_fd, "}\n");
        exception_count++;
        flush_result();
    endtask

    task automatic unavailable_checkpoint(input int slot, input string reason);
        $display("HDL_CHECKPOINT_UNAVAILABLE index=%0d pc=%08x reason=%s",
            slot, checkpoint_table[slot][31:0], reason);
        checkpoint_done[slot] = 1'b1;
        checkpoint_half_valid[slot] = 1'b0;
    endtask

    task automatic emit_checkpoint(
        input int slot, input logic [31:0] instruction_pc,
        input logic [31:0] opcode, input int width
    );
        int physical;
        int unsigned address;
        $display("HDL_CHECKPOINT_BEGIN index=%0d pc=%08x instruction=%08x width=%0d cycle=%0d",
            slot, instruction_pc, opcode, width, cycles);
        $write("HDL_CHECKPOINT_REGS");
        for (int r = 0; r < 15; r++) begin
            physical = active_index(r, VER_RETIRE_CPSR[4:0]);
            $write(" r%0d=%08x", r, VER_RETIRE_GPRS[physical * 32 +: 32]);
        end
        // Slot 15 is a layout hole. The next PC below comes from the actual
        // commit redirect, or a nonredirecting retirement's sequential edge.
        $write(" pc=%08x cpsr=%08x", checkpoint_next_pc, VER_RETIRE_CPSR);
        case (VER_RETIRE_CPSR[4:0])
            5'(MODE_FIQ): $write(" spsr=%08x", VER_RETIRE_SPSRS[31:0]);
            5'(MODE_IRQ): $write(" spsr=%08x", VER_RETIRE_SPSRS[63:32]);
            5'(MODE_SUPERVISOR): $write(" spsr=%08x", VER_RETIRE_SPSRS[95:64]);
            5'(MODE_ABORT): $write(" spsr=%08x", VER_RETIRE_SPSRS[127:96]);
            5'(MODE_UNDEFINED): $write(" spsr=%08x", VER_RETIRE_SPSRS[159:128]);
            default: begin end
        endcase
        $write("\n");
        $display("HDL_CHECKPOINT_MEMORY_CAPTURE supported=true base=%08x length=%08x stride=00000004 count=%0d",
            memory_base, memory_length, memory_length / 4);
        for (int offset = 0; offset < memory_length; offset += 4) begin
            address = memory_base + 32'(offset);
            if (address >= IWRAM_BASE && address < IWRAM_BASE + IWRAM_BYTES)
                $display("HDL_CHECKPOINT_MEM addr=%08x value=%08x", address,
                    u_iwram.mem[(address - IWRAM_BASE) >> 2]);
            else
                $display("HDL_CHECKPOINT_MEM addr=%08x value=%08x", address,
                    u_mem.mem[address >> 2]);
        end
        $display("HDL_CHECKPOINT_END index=%0d", slot);
        checkpoint_done[slot] = 1'b1;
        checkpoint_half_valid[slot] = 1'b0;
        checkpoint_observed++;
    endtask

    task automatic emit_result(input bit complete);
        int physical;
        int unsigned address;
        logic [31:0] assertion_receipt;
        string status;
        if (complete) status = "completed";
        else status = "timeout";
        if (guest_word_count != 0)
            $fatal(1, "[standalone] incomplete guest exception console frame");
        if (checkpoint_count != 0) begin
            for (int slot = 0; slot < checkpoint_count; slot++)
                if (checkpoint_half_valid[slot] && !checkpoint_done[slot])
                    unavailable_checkpoint(slot, "incomplete_thumb_instruction");
            $display("HDL_CHECKPOINT_CAPTURE supported=true requested=%0d observed=%0d closed=%0s source=rtl_retirement_port",
                checkpoint_count, checkpoint_observed, complete ? "true" : "false");
        end
        finished = 1'b1;
        $fwrite(result_fd,
            "{\"kind\":\"result\",\"backend\":\"arm7tdmi-sv\",\"status\":\"%s\",\"complete\":%s,\"cycles\":%0d,\"retired_count\":%0d,\"exception_count\":%0d,\"exception_log_count\":%0d,\"exception_dropped_count\":0,\"last_retired_pc\":",
            status, complete ? "true" : "false",
            cycles, retired_count, exception_count, exception_count);
        if (retired_count != 0) $fwrite(result_fd, "\"0x%08x\"", last_retired_pc);
        else $fwrite(result_fd, "null");
        // The completion contract stops only on a normally retired sequential
        // NOP.  Its observed retirement PC and ISA state identify the next
        // instruction boundary.  This is separate from the unobservable
        // physical R15 storage slot below.
        if (complete)
            $fwrite(result_fd,
                ",\"next_pc\":\"0x%08x\",\"next_pc_source\":\"sequential_completion_retirement\"",
                last_retired_pc + (last_retired_thumb ? 32'd2 : 32'd4));
        else
            $fwrite(result_fd, ",\"next_pc\":null,\"next_pc_source\":\"unavailable\"");
        $fwrite(result_fd, ",\"registers\":{");
        for (int r = 0; r < 15; r++) begin
            physical = active_index(r, VER_RETIRE_CPSR[4:0]);
            $fwrite(result_fd, "%s\"r%0d\":\"0x%08x\"", r == 0 ? "" : ",",
                r, VER_RETIRE_GPRS[physical * 32 +: 32]);
        end
        // VER_RETIRE_GPRS slot 15 is a layout hole, not the PC register.
        $fwrite(result_fd,
            ",\"r15\":null},\"r15_availability\":\"unavailable\",\"cpsr\":\"0x%08x\",\"spsrs\":[",
            VER_RETIRE_CPSR);
        for (int r = 0; r < 5; r++)
            $fwrite(result_fd, "%s\"0x%08x\"", r == 0 ? "" : ",",
                VER_RETIRE_SPSRS[r * 32 +: 32]);
        $fwrite(result_fd,
            "],\"memory\":{\"address\":\"0x%08x\",\"length\":%0d,\"bytes\":\"",
            memory_base, memory_length);
        for (int offset = 0; offset < memory_length; offset++) begin
            address = memory_base + 32'(offset);
            if (address >= IWRAM_BASE && address < IWRAM_BASE + IWRAM_BYTES)
                $fwrite(result_fd, "%02x", u_iwram.mem[(address - IWRAM_BASE) >> 2][int'(address[1:0]) * 8 +: 8]);
            else
                $fwrite(result_fd, "%02x", u_mem.mem[address >> 2][int'(address[1:0]) * 8 +: 8]);
        end
        $fwrite(result_fd, "\"}");
        if (assertion_result_enabled) begin
            if (assertion_result_address >= IWRAM_BASE
                && assertion_result_address < IWRAM_BASE + IWRAM_BYTES)
                assertion_receipt = u_iwram.mem[(assertion_result_address - IWRAM_BASE) >> 2];
            else
                assertion_receipt = u_mem.mem[assertion_result_address >> 2];
            $fwrite(result_fd,
                ",\"guest_assertion_receipt_address\":\"0x%08x\",\"guest_assertion_receipt\":\"0x%08x\"",
                assertion_result_address, assertion_receipt);
        end
        $fwrite(result_fd, "}\n");
        flush_result();
        $fclose(result_fd);
    endtask

    initial forever begin
        @(posedge CLK);
        // Observe the redirect selected for this enabled commit before NBA
        // changes the core's live state. This never drives a CPU signal.
        commit_flush = u_dut.u_core.flush;
        commit_flush_pc = u_dut.u_core.flush_target_pc;
        // ARM7's address phase precedes the accepted data by one cycle.
        // Observe the same completed write as the behavioral memory model.
        if (nRESET && !finished && response_write && !ABORT
            && (response_trans == 2'(TRANS_N) || response_trans == 2'(TRANS_S))
            && response_address == GUEST_CONSOLE) begin
            if (response_size != 2'(SIZE_WORD))
                $fatal(1, "[standalone] guest console requires word stores");
            $display("HDL_GUEST_WORD value=%08x", WDATA);
            guest_frame[guest_word_count] = WDATA;
            guest_word_count++;
            if (guest_word_count == 8) begin
                emit_exception();
                guest_word_count = 0;
            end
        end
        #1;
        if (nRESET && !finished) begin
            cycles++;
            if (VER_RETIRE_VALID) begin
                retired_count++;
                last_retired_pc = VER_RETIRE_PC;
                last_retired_thumb = VER_RETIRE_THUMB;
                // LDM-to-PC commits the redirect one cycle before its final
                // retirement; the existing debug observer retains that actual
                // target until the destination starts. Direct PC writes and
                // LDR-to-PC also have the same-edge flush sampled above.
                if (commit_flush)
                    checkpoint_next_pc = commit_flush_pc;
                else if (u_dut.ice_core_pc_redirect_pending)
                    checkpoint_next_pc = u_dut.ice_core_pc_redirect_pc;
                else
                    checkpoint_next_pc = VER_RETIRE_PC + (VER_RETIRE_THUMB ? 32'd2 : 32'd4);
                for (int slot = 0; slot < checkpoint_count; slot++) begin
                    if (!checkpoint_done[slot]) begin
                        if (checkpoint_half_valid[slot]) begin
                            // A four-byte classic Thumb BL is two real
                            // halfword retirements. Join only consecutive
                            // observations of those exact instruction PCs.
                            if (VER_RETIRE_THUMB && !VER_RETIRE_EXCEPTION_VALID
                                && VER_RETIRE_PC == checkpoint_table[slot][31:0] + 32'd2)
                                emit_checkpoint(slot, checkpoint_table[slot][31:0],
                                    {VER_RETIRE_OPCODE[15:0], checkpoint_first_half[slot]}, 4);
                            else
                                unavailable_checkpoint(slot, "nonconsecutive_thumb_retirement");
                        end else if (VER_RETIRE_PC == checkpoint_table[slot][31:0]) begin
                            if (VER_RETIRE_EXCEPTION_VALID)
                                unavailable_checkpoint(slot, "exception_entry_boundary");
                            else if (VER_RETIRE_THUMB && checkpoint_table[slot][63:32] == 4) begin
                                checkpoint_first_half[slot] = VER_RETIRE_OPCODE[15:0];
                                checkpoint_half_valid[slot] = 1'b1;
                            end else if (checkpoint_table[slot][63:32] == (VER_RETIRE_THUMB ? 2 : 4))
                                emit_checkpoint(slot, VER_RETIRE_PC, VER_RETIRE_OPCODE,
                                    VER_RETIRE_THUMB ? 2 : 4);
                            else
                                unavailable_checkpoint(slot, "retirement_width_mismatch");
                        end
                    end
                end
            end
            if (VER_RETIRE_VALID && VER_RETIRE_PC == stop_pc
                && !VER_RETIRE_EXCEPTION_VALID && sequential_completion_nop()) begin
                emit_result(1'b1);
                $finish;
            end else if (cycles >= 64'(max_cycles)) begin
                emit_result(1'b0);
                $fatal(1, "[standalone] cycle limit reached");
            end
        end
    end

    initial begin
        int program_fd;
        memory_base = 32'h10000;
        if (!$value$plusargs("PROGRAM_HEX=%s", program_hex))
            $fatal(1, "[standalone] missing +PROGRAM_HEX");
        if (!$value$plusargs("RESULT_JSONL=%s", result_jsonl))
            $fatal(1, "[standalone] missing +RESULT_JSONL");
        if (!$value$plusargs("STOP_PC=%h", stop_pc))
            $fatal(1, "[standalone] missing +STOP_PC (hexadecimal)");
        void'($value$plusargs("MEMORY_BASE=%h", memory_base));
        void'($value$plusargs("MEMORY_LENGTH=%d", memory_length));
        void'($value$plusargs("MAX_CYCLES=%d", max_cycles));
        void'($value$plusargs("CHECKPOINT_COUNT=%d", checkpoint_count));
        if (checkpoint_count < 0 || checkpoint_count > 65536)
            $fatal(1, "[standalone] invalid checkpoint count");
        if (checkpoint_count != 0) begin
            if (!$value$plusargs("CHECKPOINTS=%s", checkpoint_file))
                $fatal(1, "[standalone] missing CHECKPOINTS");
            program_fd = $fopen(checkpoint_file, "r");
            if (program_fd == 0) $fatal(1, "[standalone] cannot open CHECKPOINTS");
            $fclose(program_fd);
            $readmemh(checkpoint_file, checkpoint_table, 0, checkpoint_count - 1);
            for (int slot = 0; slot < checkpoint_count; slot++) begin
                checkpoint_done[slot] = 1'b0;
                checkpoint_half_valid[slot] = 1'b0;
                if (!(checkpoint_table[slot][63:32] == 2 || checkpoint_table[slot][63:32] == 4))
                    $fatal(1, "[standalone] invalid checkpoint width");
            end
        end
        assertion_result_enabled = $value$plusargs(
            "ASSERTION_RESULT_ADDRESS=%h", assertion_result_address);
        if (MEMORY_WORDS <= 0 || (MEMORY_WORDS & (MEMORY_WORDS - 1)) != 0)
            $fatal(1, "[standalone] MEMORY_WORDS must be a positive power of two");
        if (stop_pc >= 32'(MEMORY_BYTES) || stop_pc[0])
            $fatal(1, "[standalone] STOP_PC must be a halfword-aligned RAM address");
        if (max_cycles <= 0 || memory_length <= 0
            || !(64'(memory_base) + 64'(memory_length) <= 64'(MEMORY_BYTES)
                || (memory_base >= IWRAM_BASE
                    && 64'(memory_base) + 64'(memory_length) <= 64'(IWRAM_BASE) + 64'(IWRAM_BYTES))))
            $fatal(1, "[standalone] invalid cycle limit or memory window");
        if (assertion_result_enabled
            && (assertion_result_address[1:0] != 2'b00
                || !(assertion_result_address < 32'(MEMORY_BYTES)
                    || (assertion_result_address >= IWRAM_BASE
                        && assertion_result_address < IWRAM_BASE + IWRAM_BYTES))))
            $fatal(1, "[standalone] invalid guest assertion receipt address");
        // Some simulators only warn when $readmemh cannot open its input.
        // Reject that case before executing the sparse ELF load.
        program_fd = $fopen(program_hex, "r");
        if (program_fd == 0) $fatal(1, "[standalone] cannot open PROGRAM_HEX");
        if ($fgetc(program_fd) == -1) $fatal(1, "[standalone] empty PROGRAM_HEX");
        $fclose(program_fd);
        $readmemh(program_hex, u_mem.mem);
        if ($value$plusargs("IWRAM_HEX=%s", iwram_hex)) begin
            program_fd = $fopen(iwram_hex, "r");
            if (program_fd == 0) $fatal(1, "[standalone] cannot open IWRAM_HEX");
            if ($fgetc(program_fd) == -1) $fatal(1, "[standalone] empty IWRAM_HEX");
            $fclose(program_fd);
            $readmemh(iwram_hex, u_iwram.mem);
        end
        result_fd = $fopen(result_jsonl, "w");
        if (result_fd == 0) $fatal(1, "[standalone] cannot open RESULT_JSONL");
        $fwrite(result_fd,
            "{\"kind\":\"header\",\"schema\":\"arm7tdmis-standalone-v1\",\"backend\":\"arm7tdmi-sv\",\"execution_backend\":\"arm7tdmi-sv\",\"reference_only\":false,\"byte_order\":\"little\",\"raw_reason_code_namespace\":\"arm7tdmis.exception_e-v1\",\"exception_source\":\"guest_asm_console_v1\",\"stop_pc\":\"0x%08x\",\"memory_capacity\":%0d,\"iwram_base\":%0d,\"iwram_capacity\":%0d,\"iwram_independent\":true}\n",
            stop_pc, MEMORY_BYTES, IWRAM_BASE, IWRAM_BYTES);
        $display("HDL_GUEST_CONSOLE protocol=guest_asm_console_v1 address=0000ff00");
        flush_result();
        repeat (4) @(posedge CLK);
        @(negedge CLK);
        nRESET = 1'b1;
    end

    wire _unused = &{1'b0, CPnMREQ, CPSEQ, CPnTRANS, CPnOPC, CPTBIT, CPnI,
        DBGACK, DBGnEXEC, DBGINSTRVALID, DBGRNG, DBGCOMMTX, DBGCOMMRX,
        DBGTDO, DBGnTDOEN, DMORE, VER_RETIRE_CONDITION_PASS,
        VER_RETIRE_INJECTED, VER_RETIRE_EXCEPTION, VER_RETIRE_GPRS[511:480]};
endmodule
