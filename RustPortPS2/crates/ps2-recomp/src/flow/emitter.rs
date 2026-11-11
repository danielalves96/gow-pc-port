//! Emission of one branch or jump together with its delay slot.
//!
//! Port of `ControlFlowEmitter` (`control_flow_emitter.cpp`). The text is the
//! generated C++, so indentation, hexadecimal case (`0x{:X}u` for program
//! counters, `label_{:x}` for labels) and statement order are all contractual.

use std::fmt::Write as _;

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::calls::{resolve_stub_name, resolve_syscall_name};
use crate::emit::lean::lean_statement;
use crate::flow::analyzer::AnalysisResult;
use crate::generator::CodeGenerator;
use crate::translate::control_flow::is_guest_nop;
use crate::translate::instruction::build_absolute_jump_target;
use crate::types::Function;

#[derive(Clone, Copy, PartialEq, Eq)]
enum StaticBranchKind {
    Jump,
    Call,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RegisterBranchKind {
    Jump,
    Call,
}

pub struct ControlFlowEmitter<'a> {
    generator: &'a mut CodeGenerator,
    branch: &'a Instruction,
    delay_slot: &'a Instruction,
    analysis: &'a AnalysisResult,
    delay_slot_override: String,
    out: String,
    /// GOW-Port: `[codegen] lean_pc` for this function (emit/lean.rs).
    lean: bool,
}

impl<'a> ControlFlowEmitter<'a> {
    pub fn new(
        generator: &'a mut CodeGenerator,
        branch: &'a Instruction,
        delay_slot: &'a Instruction,
        _function: &'a Function,
        analysis: &'a AnalysisResult,
        delay_slot_override: String,
    ) -> Self {
        Self { generator, branch, delay_slot, analysis, delay_slot_override, out: String::new(), lean: false }
    }

    /// GOW-Port: `[codegen] lean_pc`: leave out the pc stores nothing can read (emit/lean.rs).
    pub fn lean(mut self, lean: bool) -> Self {
        self.lean = lean;
        self
    }

    fn branch_pc(&self) -> u32 {
        self.branch.address
    }

    fn delay_pc(&self) -> u32 {
        self.branch.address.wrapping_add(4)
    }

    fn fallthrough_pc(&self) -> u32 {
        self.branch.address.wrapping_add(8)
    }

    fn has_real_delay_slot(&self) -> bool {
        !self.delay_slot_override.is_empty() || !is_guest_nop(self.delay_slot)
    }

    fn is_call_like_edge(&self) -> bool {
        self.branch.opcode == OPCODE_JAL
            || (self.branch.opcode == OPCODE_SPECIAL && self.branch.function == SPECIAL_JALR)
    }

    fn is_internal_target(&self, target: u32) -> bool {
        self.analysis.entry_points.contains(&target)
    }

    fn is_likely_branch(&self) -> bool {
        let b = self.branch;
        matches!(b.opcode, OPCODE_BEQL | OPCODE_BNEL | OPCODE_BLEZL | OPCODE_BGTZL)
            || (b.opcode == OPCODE_REGIMM
                && matches!(b.rt, REGIMM_BLTZL | REGIMM_BGEZL | REGIMM_BLTZALL | REGIMM_BGEZALL))
            || (b.opcode == OPCODE_COP1 && b.rs == COP1_BC && matches!(b.rt, COP1_BC_BCFL | COP1_BC_BCTL))
            || (b.opcode == OPCODE_COP2 && b.rs == COP2_BC && matches!(b.rt, COP2_BC_BCFL | COP2_BC_BCTL))
    }

    fn resolved_local_indirect_targets(&self) -> Vec<u32> {
        let b = self.branch;
        if b.opcode != OPCODE_SPECIAL {
            return Vec::new();
        }
        if !((b.function == SPECIAL_JR && b.rs != 31) || b.function == SPECIAL_JALR) {
            return Vec::new();
        }
        let Some(targets) = self.analysis.jump_table_targets.get(&b.address) else {
            return Vec::new();
        };
        let mut targets = targets.clone();
        targets.sort_unstable();
        targets.dedup();
        targets
    }

    fn delay_slot_code(&mut self) -> String {
        if !self.has_real_delay_slot() {
            return String::new();
        }
        if !self.delay_slot_override.is_empty() {
            return self.delay_slot_override.clone();
        }

        let mut code = String::new();
        if self.generator.emit_instruction_comments() {
            let _ = write!(code, "// 0x{:x}: 0x{:x}", self.delay_slot.address, self.delay_slot.raw);
            let disassembly = self.delay_slot.disassembly();
            if !disassembly.is_empty() {
                let _ = write!(code, "  {disassembly}");
            }
            code.push_str(" (Delay Slot)\n");
        }

        code.push_str(&self.generator.translate_instruction(self.delay_slot));
        code
    }

    fn emit_delay_slot(&mut self, indent: &str) {
        if !self.has_real_delay_slot() {
            return;
        }

        // GOW-Port: lean pc: a delay slot that cannot reach the runtime needs no pc / delay-slot state.
        if self.lean && self.delay_slot_override.is_empty() {
            let translation = self.generator.translate_instruction(self.delay_slot);
            if let Some(lean_text) = lean_statement(self.delay_slot, &translation) {
                if self.generator.emit_instruction_comments() {
                    let _ = write!(self.out, "{indent}// 0x{:x}: 0x{:x}", self.delay_slot.address, self.delay_slot.raw);
                    let disassembly = self.delay_slot.disassembly();
                    if !disassembly.is_empty() {
                        let _ = write!(self.out, "  {disassembly}");
                    }
                    self.out.push_str(" (Delay Slot)\n");
                }
                for line in lean_text.split('\n') {
                    if !line.is_empty() {
                        let _ = writeln!(self.out, "{indent}{line}");
                    }
                }
                return;
            }
        }

        let _ = writeln!(self.out, "{indent}ctx->pc = 0x{:X}u;", self.delay_pc());
        let _ = writeln!(self.out, "{indent}ctx->in_delay_slot = true;");
        let _ = writeln!(self.out, "{indent}ctx->branch_pc = 0x{:X}u;", self.branch_pc());

        let code = self.delay_slot_code();
        // `std::getline` loop: split on '\n', drop empty lines.
        for line in code.split('\n') {
            if !line.is_empty() {
                let _ = writeln!(self.out, "{indent}{line}");
            }
        }

        let _ = writeln!(self.out, "{indent}ctx->in_delay_slot = false;");
    }

    fn emit_resume_from_delay_slot_entry(&mut self) {
        if !self.is_internal_target(self.delay_pc()) {
            return;
        }

        let _ = writeln!(self.out, "    if (ctx->pc == 0x{:X}u) {{", self.delay_pc());
        self.emit_delay_slot("        ");
        let _ = writeln!(self.out, "        ctx->pc = 0x{:X}u;", self.fallthrough_pc());

        if self.is_internal_target(self.fallthrough_pc()) {
            let _ = writeln!(self.out, "        goto label_{:x};", self.fallthrough_pc());
        } else {
            let _ = writeln!(self.out, "        goto label_fallthrough_0x{:x};", self.branch_pc());
        }

        self.out.push_str("    }\n");
    }

    fn emit_internal_target(&mut self, target: u32, source_pc: u32, indent: &str) {
        let back_edge = target <= source_pc && !self.is_call_like_edge();
        // GOW-Port: lean pc: only a back edge's checkpoint can read the pc (it resumes at the target).
        if !self.lean || back_edge {
            let _ = writeln!(self.out, "{indent}ctx->pc = 0x{target:X}u;");
        }
        if back_edge {
            let _ = writeln!(self.out, "{indent}if (runtime->eeCheckpointDue()) {{");
            let _ = writeln!(self.out, "{indent}    return;");
            let _ = writeln!(self.out, "{indent}}}");
        }
        let _ = writeln!(self.out, "{indent}goto label_{target:x};");
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_runtime_branch_dispatch(
        &mut self,
        target_expression: &str,
        source_pc: u32,
        return_pc: u32,
        runtime_kind: &str,
        debug_name: &str,
        indent: &str,
        return_on_transfer: bool,
    ) {
        let _ = writeln!(
            self.out,
            "{indent}if (!runtime->dispatchGuestBranch(rdram, ctx, {target_expression}, 0x{source_pc:X}u, 0x{return_pc:X}u, PS2Runtime::GuestBranchKind::{runtime_kind}, \"{debug_name}\")) {{"
        );
        if return_on_transfer {
            let _ = writeln!(self.out, "{indent}    return;");
        }
        let _ = writeln!(self.out, "{indent}}}");
    }

    fn emit_direct_function_jump_if_available(&mut self, target: u32, kind: StaticBranchKind, indent: &str) -> bool {
        if kind != StaticBranchKind::Jump {
            return false;
        }

        let function_name = self.generator.get_function_name(target);
        if function_name.is_empty() {
            return false;
        }

        let _ = writeln!(self.out, "{indent}{function_name}(rdram, ctx, runtime); return;");
        true
    }

    fn emit_external_jump_dispatch(&mut self, target: u32, kind: StaticBranchKind, indent: &str) {
        let is_call = kind == StaticBranchKind::Call;
        let branch_pc = self.branch_pc();
        let fallthrough = self.fallthrough_pc();
        self.emit_runtime_branch_dispatch(
            &format!("0x{target:X}u"),
            branch_pc,
            if is_call { fallthrough } else { 0 },
            if is_call { "DirectCall" } else { "DirectJump" },
            if is_call { "JAL" } else { "J" },
            indent,
            true,
        );
    }

    fn emit_external_register_call_dispatch(&mut self, jump_target_expression: &str, indent: &str) {
        let branch_pc = self.branch_pc();
        let fallthrough = self.fallthrough_pc();
        self.emit_runtime_branch_dispatch(
            jump_target_expression,
            branch_pc,
            fallthrough,
            "IndirectCall",
            "JALR",
            indent,
            true,
        );
    }

    fn emit_external_register_jump_dispatch(
        &mut self,
        jump_target_expression: &str,
        kind: RegisterBranchKind,
        rs_reg: u32,
        indent: &str,
    ) {
        let is_return = kind == RegisterBranchKind::Jump && rs_reg == 31;

        if is_return {
            let _ = writeln!(
                self.out,
                "{indent}#if defined(PS2X_STRICT_RETURN_DIAGNOSTICS) && PS2X_STRICT_RETURN_DIAGNOSTICS"
            );
            let _ = writeln!(
                self.out,
                "{indent}(void)runtime->dispatchGuestBranch(rdram, ctx, {jump_target_expression}, 0x{:X}u, 0u, PS2Runtime::GuestBranchKind::Return, \"JR $ra\");",
                self.branch_pc()
            );
            let _ = writeln!(self.out, "{indent}return;");
            let _ = writeln!(self.out, "{indent}#else");
            let _ = writeln!(self.out, "{indent}ctx->pc = {jump_target_expression};");
            let _ = writeln!(self.out, "{indent}return;");
            let _ = writeln!(self.out, "{indent}#endif");
            return;
        }

        let branch_pc = self.branch_pc();
        self.emit_runtime_branch_dispatch(jump_target_expression, branch_pc, 0, "IndirectJump", "JR", indent, true);
    }

    fn emit_relocation_call_if_available(&mut self, kind: StaticBranchKind, indent: &str) -> bool {
        let Some(name) = self.generator.relocation_call_name(self.branch.address) else {
            return false;
        };
        if name.is_empty() {
            return false;
        }

        let resolved_syscall = resolve_syscall_name(name);
        let resolved_stub = resolve_stub_name(name);
        if resolved_syscall.is_none() && resolved_stub.is_none() {
            return false;
        }

        let is_syscall = resolved_syscall.is_some();
        let handler_name = resolved_syscall.or(resolved_stub).unwrap_or_default();

        let _ = writeln!(self.out, "{indent}{{");
        if kind == StaticBranchKind::Call {
            let _ = writeln!(self.out, "{indent}    ctx->pc = 0x{:X}u;", self.fallthrough_pc());
        } else {
            let _ = writeln!(self.out, "{indent}    ctx->pc = getRegU32(ctx, 31);");
        }
        let _ = writeln!(
            self.out,
            "{indent}    {}{handler_name}(rdram, ctx, runtime);",
            if is_syscall { "ps2_syscalls::" } else { "ps2_stubs::" }
        );
        let _ = writeln!(self.out, "{indent}}}");

        if kind == StaticBranchKind::Jump {
            let _ = writeln!(self.out, "{indent}return;");
        } else {
            let _ = writeln!(self.out, "{indent}if (ctx->pc != 0x{:X}u) {{ return; }}", self.fallthrough_pc());
        }

        true
    }

    fn emit_static_jump(&mut self, kind: StaticBranchKind) {
        if kind == StaticBranchKind::Call {
            let _ = writeln!(self.out, "    SET_GPR_U32(ctx, 31, 0x{:X}u);", self.fallthrough_pc());
        }

        self.emit_delay_slot("    ");

        let target = build_absolute_jump_target(self.branch.address, self.branch.target);
        if self.is_internal_target(target) {
            let branch_pc = self.branch_pc();
            self.emit_internal_target(target, branch_pc, "    ");
            return;
        }

        // GOW-Port: lean pc: a call's dispatch, and the relocation stub path, store the pc themselves. A jump keeps it: a
        // tail call enters the callee directly, and the callee's resume switch reads it.
        if !self.lean || kind != StaticBranchKind::Call {
            let _ = writeln!(self.out, "    ctx->pc = 0x{target:X}u;");
        }

        if kind == StaticBranchKind::Call && self.emit_relocation_call_if_available(kind, "    ") {
            return;
        }

        if self.emit_direct_function_jump_if_available(target, kind, "    ") {
            return;
        }

        self.emit_external_jump_dispatch(target, kind, "    ");
    }

    fn emit_register_jump(&mut self, kind: RegisterBranchKind) {
        let rs_reg = self.branch.rs;
        let rd_reg = self.branch.rd;
        let sorted_internal_targets = self.resolved_local_indirect_targets();

        self.out.push_str("    {\n");
        let _ = writeln!(self.out, "        const uint32_t jumpTarget = GPR_U32(ctx, {rs_reg});");

        if kind == RegisterBranchKind::Call && rd_reg != 0 {
            let _ = writeln!(self.out, "        SET_GPR_U32(ctx, {rd_reg}, 0x{:X}u);", self.fallthrough_pc());
        }

        self.emit_delay_slot("        ");
        self.out.push_str("        ctx->pc = jumpTarget;\n");

        if !sorted_internal_targets.is_empty() {
            self.out.push_str("        switch (jumpTarget) {\n");
            for target in &sorted_internal_targets {
                let _ = writeln!(self.out, "            case 0x{target:X}u: goto label_{target:x};");
            }
            self.out.push_str("            default: break;\n");
            self.out.push_str("        }\n");
        }

        if kind == RegisterBranchKind::Jump {
            self.emit_external_register_jump_dispatch("jumpTarget", kind, rs_reg, "        ");
        } else {
            self.emit_external_register_call_dispatch("jumpTarget", "        ");
        }

        self.out.push_str("    }\n");
    }

    fn conditional_branch_expression(&self) -> String {
        let b = self.branch;
        let rs = b.rs;
        let rt = b.rt;

        match b.opcode {
            OPCODE_BEQ | OPCODE_BEQL => return format!("GPR_U64(ctx, {rs}) == GPR_U64(ctx, {rt})"),
            OPCODE_BNE | OPCODE_BNEL => return format!("GPR_U64(ctx, {rs}) != GPR_U64(ctx, {rt})"),
            // GOW-Port: on the R5900 BLEZ/BGTZ/BLTZ/BGEZ compare the 64-bit register (SotC fork, 3c46932).
            OPCODE_BLEZ | OPCODE_BLEZL => return format!("GPR_S64(ctx, {rs}) <= 0"),
            OPCODE_BGTZ | OPCODE_BGTZL => return format!("GPR_S64(ctx, {rs}) > 0"),
            OPCODE_REGIMM => {
                return match rt {
                    REGIMM_BLTZ | REGIMM_BLTZL | REGIMM_BLTZAL | REGIMM_BLTZALL => format!("GPR_S64(ctx, {rs}) < 0"),
                    REGIMM_BGEZ | REGIMM_BGEZL | REGIMM_BGEZAL | REGIMM_BGEZALL => format!("GPR_S64(ctx, {rs}) >= 0"),
                    _ => "false".to_owned(),
                };
            }
            OPCODE_COP1 if b.rs == COP1_BC => {
                return if rt == COP1_BC_BCF || rt == COP1_BC_BCFL {
                    "!(ctx->fcr31 & 0x800000)".to_owned()
                } else {
                    "(ctx->fcr31 & 0x800000)".to_owned()
                };
            }
            OPCODE_COP2 if b.rs == COP2_BC => {
                return if rt == COP2_BC_BCF || rt == COP2_BC_BCFL {
                    "!(ctx->vu0_status & 0x1)".to_owned()
                } else {
                    "(ctx->vu0_status & 0x1)".to_owned()
                };
            }
            _ => {}
        }

        "false".to_owned()
    }

    fn conditional_branch_target(&self) -> u32 {
        let offset_bytes = (self.branch.simmediate as u16 as i16 as i32) << 2;
        self.branch.address.wrapping_add(4).wrapping_add(offset_bytes as u32)
    }

    fn emit_conditional_branch(&mut self) {
        let target = self.conditional_branch_target();
        let likely = self.is_likely_branch();
        let branch_taken_var = format!("branch_taken_0x{:x}", self.branch.address);
        let mut unconditional_link_code = String::new();
        let mut conditional_link_code = String::new();

        if self.branch.opcode == OPCODE_REGIMM {
            if self.branch.rt == REGIMM_BLTZAL || self.branch.rt == REGIMM_BGEZAL {
                unconditional_link_code = format!("SET_GPR_U32(ctx, 31, 0x{:X}u);", self.fallthrough_pc());
            } else if self.branch.rt == REGIMM_BLTZALL || self.branch.rt == REGIMM_BGEZALL {
                conditional_link_code = format!("SET_GPR_U32(ctx, 31, 0x{:X}u);", self.fallthrough_pc());
            }
        }

        self.out.push_str("    {\n");
        let _ =
            writeln!(self.out, "        const bool {branch_taken_var} = ({});", self.conditional_branch_expression());

        if !unconditional_link_code.is_empty() {
            let _ = writeln!(self.out, "        {unconditional_link_code}");
        }

        let branch_pc = self.branch_pc();
        if likely {
            let _ = writeln!(self.out, "        if ({branch_taken_var}) {{");
            if !conditional_link_code.is_empty() {
                let _ = writeln!(self.out, "            {conditional_link_code}");
            }
            self.emit_delay_slot("            ");

            if self.is_internal_target(target) {
                self.emit_internal_target(target, branch_pc, "            ");
            } else {
                let _ = writeln!(self.out, "            ctx->pc = 0x{target:X}u;");
                self.out.push_str("            return;\n");
            }
            self.out.push_str("        }\n");
        } else {
            if !conditional_link_code.is_empty() {
                let _ = writeln!(self.out, "        if ({branch_taken_var}) {{ {conditional_link_code} }}");
            }

            self.emit_delay_slot("        ");

            let _ = writeln!(self.out, "        if ({branch_taken_var}) {{");
            if self.is_internal_target(target) {
                self.emit_internal_target(target, branch_pc, "            ");
            } else {
                let _ = writeln!(self.out, "            ctx->pc = 0x{target:X}u;");
                self.out.push_str("            return;\n");
            }
            self.out.push_str("        }\n");
        }

        self.out.push_str("    }\n");
    }

    fn emit_fallback_instruction(&mut self) {
        let code = self.generator.translate_instruction(self.branch);
        let _ = writeln!(self.out, "    {code}");
        self.emit_delay_slot("    ");
    }

    fn emit_fallthrough_label_if_needed(&mut self) {
        if self.is_internal_target(self.delay_pc()) && !self.is_internal_target(self.fallthrough_pc()) {
            let _ = writeln!(self.out, "label_fallthrough_0x{:x}:", self.branch_pc());
        }
    }

    fn emit_final_fallthrough(&mut self) {
        // GOW-Port: lean pc: nothing reads the pc before the next instruction stores what it needs.
        if self.lean {
            return;
        }
        let _ = writeln!(self.out, "    ctx->pc = 0x{:X}u;", self.fallthrough_pc());
    }

    /// `ControlFlowEmitter::emit`.
    pub fn emit(mut self) -> String {
        self.emit_resume_from_delay_slot_entry();
        let b = self.branch;
        let is_static_jump = b.opcode == OPCODE_J || b.opcode == OPCODE_JAL;
        let is_register_jump = b.opcode == OPCODE_SPECIAL && (b.function == SPECIAL_JR || b.function == SPECIAL_JALR);
        // GOW-Port: lean pc: a branch's own pc is read by nothing but the fallback (an instruction translated as such).
        if !self.lean || !(is_static_jump || is_register_jump || b.is_branch) {
            let _ = writeln!(self.out, "    ctx->pc = 0x{:X}u;", self.branch_pc());
        }

        if b.opcode == OPCODE_J || b.opcode == OPCODE_JAL {
            self.emit_static_jump(if b.opcode == OPCODE_JAL { StaticBranchKind::Call } else { StaticBranchKind::Jump });
        } else if b.opcode == OPCODE_SPECIAL && (b.function == SPECIAL_JR || b.function == SPECIAL_JALR) {
            self.emit_register_jump(if b.function == SPECIAL_JALR {
                RegisterBranchKind::Call
            } else {
                RegisterBranchKind::Jump
            });
        } else if b.is_branch {
            self.emit_conditional_branch();
        } else {
            self.emit_fallback_instruction();
        }

        self.emit_fallthrough_label_if_needed();
        self.emit_final_fallthrough();
        self.out
    }
}
