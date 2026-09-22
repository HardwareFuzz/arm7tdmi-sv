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
    always_ff @(posedge CLK) begin
        if (!nRESET) response_address <= 32'b0;
        else response_address <= ADDR;
    end
    arm7tdmis_memory #(.WORDS(MEMORY_WORDS)) u_mem (
        .CLK, .CLKEN(1'b1), .nRESET, .CFGBIGEND(1'b0),
        .ADDR, .WRITE, .SIZE, .PROT, .LOCK, .TRANS, .WDATA, .RDATA,
        .ABORT, .inject_abort(response_address >= 32'(MEMORY_BYTES))
    );

    string program_hex, result_jsonl;
    int result_fd;
    logic [31:0] stop_pc, memory_base;
    int signed memory_length = 128;
    longint signed max_cycles = 1000000;
    longint unsigned cycles = 0;
    longint unsigned retired_count = 0;
    longint unsigned exception_count = 0;
    logic [31:0] last_retired_pc = 0;
    bit finished = 1'b0;

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
        faulting = VER_RETIRE_VALID
            && VER_RETIRE_EXCEPTION != 3'(EXC_IRQ)
            && VER_RETIRE_EXCEPTION != 3'(EXC_FIQ)
            && VER_RETIRE_EXCEPTION != 3'(EXC_RESET);
        $fwrite(result_fd,
            "{\"kind\":\"exception\",\"backend\":\"arm7tdmi-sv\",\"sequence\":%0d,\"cycle\":%0d,\"raw_reason_code\":%0d,\"cause\":\"%s\",\"faulting_instruction\":%s,\"pc\":",
            exception_count, cycles, VER_RETIRE_EXCEPTION,
            cause_name(VER_RETIRE_EXCEPTION), faulting ? "true" : "false");
        if (faulting) $fwrite(result_fd, "\"0x%08x\"", VER_RETIRE_PC);
        else $fwrite(result_fd, "null");
        // A failed fetch has a fault PC but no successfully fetched opcode.
        if (faulting && VER_RETIRE_EXCEPTION != 3'(EXC_PREFETCH_ABORT)) begin
            $fwrite(result_fd,
                ",\"thumb\":%s,\"instruction_width\":%0d,\"instruction\":\"0x%08x\",\"instruction_bytes\":\"",
                VER_RETIRE_THUMB ? "true" : "false",
                VER_RETIRE_THUMB ? 2 : 4, VER_RETIRE_OPCODE);
            for (int b = 0; b < (VER_RETIRE_THUMB ? 2 : 4); b++)
                $fwrite(result_fd, "%02x", VER_RETIRE_OPCODE[b * 8 +: 8]);
            $fwrite(result_fd, "\"");
        end else begin
            $fwrite(result_fd,
                ",\"thumb\":null,\"instruction_width\":null,\"instruction\":null,\"instruction_bytes\":null");
        end
        $fwrite(result_fd, "}\n");
        exception_count++;
        flush_result();
    endtask

    task automatic emit_result(input bit complete);
        int physical;
        int unsigned address;
        string status;
        if (complete) status = "completed";
        else status = "timeout";
        finished = 1'b1;
        $fwrite(result_fd,
            "{\"kind\":\"result\",\"backend\":\"arm7tdmi-sv\",\"status\":\"%s\",\"complete\":%s,\"cycles\":%0d,\"retired_count\":%0d,\"exception_count\":%0d,\"exception_log_count\":%0d,\"exception_dropped_count\":0,\"last_retired_pc\":",
            status, complete ? "true" : "false",
            cycles, retired_count, exception_count, exception_count);
        if (retired_count != 0) $fwrite(result_fd, "\"0x%08x\"", last_retired_pc);
        else $fwrite(result_fd, "null");
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
            $fwrite(result_fd, "%02x", u_mem.mem[address >> 2][int'(address[1:0]) * 8 +: 8]);
        end
        $fwrite(result_fd, "\"}}\n");
        flush_result();
        $fclose(result_fd);
    endtask

    initial forever begin
        @(posedge CLK);
        #1;
        if (nRESET && !finished) begin
            cycles++;
            if (VER_RETIRE_EXCEPTION_VALID) emit_exception();
            if (VER_RETIRE_VALID) begin
                retired_count++;
                last_retired_pc = VER_RETIRE_PC;
            end
            if (VER_RETIRE_VALID && VER_RETIRE_PC == stop_pc
                && !VER_RETIRE_EXCEPTION_VALID) begin
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
        if (MEMORY_WORDS <= 0 || (MEMORY_WORDS & (MEMORY_WORDS - 1)) != 0)
            $fatal(1, "[standalone] MEMORY_WORDS must be a positive power of two");
        if (stop_pc >= 32'(MEMORY_BYTES) || stop_pc[0])
            $fatal(1, "[standalone] STOP_PC must be a halfword-aligned RAM address");
        if (max_cycles <= 0 || memory_length <= 0
            || 64'(memory_base) + 64'(memory_length) > 64'(MEMORY_BYTES))
            $fatal(1, "[standalone] invalid cycle limit or memory window");
        // Some simulators only warn when $readmemh cannot open its input.
        // Reject that case before executing the zero-initialized RAM.
        program_fd = $fopen(program_hex, "r");
        if (program_fd == 0) $fatal(1, "[standalone] cannot open PROGRAM_HEX");
        if ($fgetc(program_fd) == -1) $fatal(1, "[standalone] empty PROGRAM_HEX");
        $fclose(program_fd);
        for (int word = 0; word < MEMORY_WORDS; word++) u_mem.mem[word] = 0;
        $readmemh(program_hex, u_mem.mem);
        result_fd = $fopen(result_jsonl, "w");
        if (result_fd == 0) $fatal(1, "[standalone] cannot open RESULT_JSONL");
        $fwrite(result_fd,
            "{\"kind\":\"header\",\"schema\":\"arm7tdmis-standalone-v1\",\"backend\":\"arm7tdmi-sv\",\"execution_backend\":\"arm7tdmi-sv\",\"reference_only\":false,\"byte_order\":\"little\",\"raw_reason_code_namespace\":\"arm7tdmis.exception_e-v1\",\"stop_pc\":\"0x%08x\",\"memory_capacity\":%0d}\n",
            stop_pc, MEMORY_BYTES);
        flush_result();
        repeat (4) @(posedge CLK);
        @(negedge CLK);
        nRESET = 1'b1;
    end

    wire _unused = &{1'b0, CPnMREQ, CPSEQ, CPnTRANS, CPnOPC, CPTBIT, CPnI,
        DBGACK, DBGnEXEC, DBGINSTRVALID, DBGRNG, DBGCOMMTX, DBGCOMMRX,
        DBGTDO, DBGnTDOEN, DMORE, VER_RETIRE_CONDITION_PASS,
        VER_RETIRE_INJECTED, VER_RETIRE_GPRS[511:480]};
endmodule
