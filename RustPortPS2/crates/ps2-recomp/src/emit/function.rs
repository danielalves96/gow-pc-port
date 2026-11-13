//! Emission of one recompiled function.
//!
//! Port of `FunctionEmitter::emit` (`function_emitter.cpp`): the file header, the
//! resume switch, one labelled statement per instruction (with its disassembly
//! comment), the branch/delay-slot groups and the GIF DMA kick replacement.

use std::fmt::Write as _;

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::emit::lean::{lean_statement, Lean};
use crate::emit::locals::{build_body, Chunk, ChunkKind};
use crate::emit::ramgroup::{goto_targets, RamGroups};
use crate::flow::emitter::ControlFlowEmitter;
use crate::generator::{CodeGenerator, GenerationError};
use crate::gif_dma::{
    emit_gif_dma_capture, gif_dma_delay_slot_override, gif_dma_kick_call, resolve_memory_access_hint,
    try_build_gif_dma_kick_plan, update_constant_registers, ConstantRegisterState, GifDmaKickPlan,
};
use crate::translate::instruction::FamilyTranslator;
use crate::types::Function;

fn make_synthetic_delay_slot(address: u32) -> Instruction {
    Instruction { address, raw: 0, opcode: OPCODE_SPECIAL, function: SPECIAL_SLL, ..Instruction::default() }
}

/// `CodeGenerator::generateFunction`.
pub fn emit_function(
    cg: &mut CodeGenerator,
    function: &Function,
    instructions: &[Instruction],
    use_headers: bool,
) -> Result<String, GenerationError> {
    let mut ss = String::new();
    cg.set_current_function_name(&function.name);

    if use_headers {
        ss.push_str("#include <stdexcept>\n");
        ss.push_str("#include \"ps2_runtime_macros.h\"\n");
        ss.push_str("#include \"ps2_runtime.h\"\n");
        ss.push_str("#include <ps2_recompiled_functions.h>\n");
        ss.push_str("#include <ps2_recompiled_stubs.h>\n\n");
        ss.push_str("#include \"ps2_syscalls.h\"\n");
        ss.push_str("#include \"ps2_stubs.h\"\n\n");
        ss.push_str("#ifdef PS2_FUNCTION_LOG_TRACKER\n");
        ss.push_str("#include \"ps2_log.h\"\n");
        ss.push_str("#endif\n\n");
    }

    let mut analysis = cg.collect_internal_branch_targets(function, instructions, None);
    let mut resume_targets: Vec<u32> = analysis.resume_entry_points.iter().copied().collect();
    if let Some(extra) = cg.resume_entry_targets_by_owner().get(&function.start) {
        resume_targets.extend_from_slice(extra);
    }
    resume_targets.sort_unstable();
    resume_targets.dedup();
    for &target in &resume_targets {
        analysis.entry_points.insert(target);
    }

    let mut constant_registers = ConstantRegisterState::new();
    let mut gif_dma_kick_plan = GifDmaKickPlan::default();
    let function_text_start = ss.len(); // GOW-Port: where a locals body puts its include
    let _ = writeln!(ss, "// Function: {}", function.name);
    let _ = writeln!(ss, "// Address: 0x{:x} - 0x{:x}", function.start, function.end);

    let mut sanitized_name = cg.get_function_name(function.start);
    if sanitized_name.is_empty() {
        sanitized_name = format!("Errorfunc_{:x}", function.start);
    }

    // GOW-Port (decision D2): `[codegen] restrict_ctx` and `lean_pc` (emit/lean.rs). Without `delay_slot_labels`, a function
    // with a branch whose delay slot is itself a branch target keeps every pc store: its delay-slot entry tests `ctx->pc`.
    let codegen = cg.codegen();
    let restrict = if codegen.restrict_ctx { "__restrict " } else { "" };
    let delay_slot_entry = instructions
        .iter()
        .any(|inst| inst.has_delay_slot && analysis.entry_points.contains(&inst.address.wrapping_add(4)));
    let lean: Option<Lean> =
        (codegen.lean_pc && !codegen.register_locals && (codegen.delay_slot_labels || !delay_slot_entry))
            .then_some(Lean { cop2: codegen.lean_cop2, delay_slot_labels: codegen.delay_slot_labels });
    let _ = writeln!(ss, "void {sanitized_name}(uint8_t* rdram, R5900Context* {restrict}ctx, PS2Runtime *runtime) {{");
    ss.push_str("#ifdef PS2_FUNCTION_LOG_TRACKER\n");
    let _ = writeln!(ss, "    PS_LOG_ENTRY(\"{sanitized_name}\");");
    ss.push_str("#endif\n");
    ss.push('\n');
    // GOW-Port (decision D2): the body is collected in chunks; the classic output is their plain concatenation.
    let mut head = String::new();
    if !resume_targets.is_empty() {
        head.push_str("    switch (ctx->pc) {\n");
        for target in &resume_targets {
            let _ = writeln!(head, "        case 0x{target:x}u: goto label_{target:x};");
        }
        head.push_str("        default: break;\n");
        head.push_str("    }\n\n");
    }
    let _ = writeln!(head, "    ctx->pc = 0x{:x}u;", function.start);
    head.push('\n');
    let mut chunks: Vec<Chunk> = Vec::new();
    fn push_chunk(chunks: &mut Vec<Chunk>, kind: ChunkKind, text: String) {
        if !text.is_empty() {
            chunks.push(Chunk { kind, text });
        }
    }
    // GOW-Port: `ram_groups` (emit/ramgroup.rs); the labels a `goto` can reach end a group.
    let mut groups = (codegen.ram_groups && !codegen.register_locals).then(RamGroups::new);
    let reachable_labels = goto_targets(instructions, analysis.jump_table_targets.values().flatten().copied());

    let mut last_instruction_was_control_flow = false;
    let emit_comments = cg.emit_instruction_comments();

    let mut i = 0usize;
    while i < instructions.len() {
        let inst = &instructions[i];
        last_instruction_was_control_flow = inst.has_delay_slot;

        let mut label = String::new();
        if analysis.entry_points.contains(&inst.address) {
            if let Some(groups) = groups.as_mut().filter(|_| reachable_labels.contains(&inst.address)) {
                groups.close_all(&mut chunks);
            }
            constant_registers.clear();
            let _ = writeln!(label, "label_{:x}:", inst.address);
        }

        if emit_comments {
            let _ = write!(label, "    // 0x{:x}: 0x{:x}", inst.address, inst.raw);
            let disassembly = inst.disassembly();
            if !disassembly.is_empty() {
                let _ = write!(label, "  {disassembly}");
            }
            label.push('\n');
        }
        push_chunk(&mut chunks, ChunkKind::Label, label);

        if inst.has_delay_slot {
            if let Some(groups) = groups.as_mut() {
                groups.close_all(&mut chunks);
            }
            let has_decoded_delay_slot =
                i + 1 < instructions.len() && instructions[i + 1].address == inst.address.wrapping_add(4);

            let synthetic_delay_slot;
            let delay_slot: &Instruction = if has_decoded_delay_slot {
                &instructions[i + 1]
            } else {
                synthetic_delay_slot = make_synthetic_delay_slot(inst.address.wrapping_add(4));
                &synthetic_delay_slot
            };

            // (`delay_slot_labels`: the branch emitter puts the label in front of the delay-slot entry block itself)
            if has_decoded_delay_slot
                && analysis.entry_points.contains(&delay_slot.address)
                && !lean.is_some_and(|lean| lean.delay_slot_labels)
            {
                push_chunk(&mut chunks, ChunkKind::Label, format!("label_{:x}:\n", delay_slot.address));
            }

            if gif_dma_kick_plan.valid
                && gif_dma_kick_plan.completes_in_delay_slot
                && gif_dma_kick_plan.branch_index == i
                && has_decoded_delay_slot
            {
                let override_code = gif_dma_delay_slot_override(delay_slot, &gif_dma_kick_plan, emit_comments);
                push_chunk(
                    &mut chunks,
                    ChunkKind::Branch,
                    ControlFlowEmitter::new(cg, inst, delay_slot, function, &analysis, override_code).lean(lean).emit(),
                );
                gif_dma_kick_plan = GifDmaKickPlan::default();
            } else {
                push_chunk(
                    &mut chunks,
                    ChunkKind::Branch,
                    ControlFlowEmitter::new(cg, inst, delay_slot, function, &analysis, String::new()).lean(lean).emit(),
                );
            }

            if has_decoded_delay_slot {
                i += 1; // Skip delay slot instruction (handled inside branch logic)
            }
            constant_registers.clear();
        } else {
            if !gif_dma_kick_plan.valid {
                gif_dma_kick_plan =
                    try_build_gif_dma_kick_plan(instructions, i, &constant_registers, &analysis.entry_points);
            }

            if gif_dma_kick_plan.suppresses(i) {
                if let Some(groups) = groups.as_mut() {
                    groups.close_all(&mut chunks);
                }
                let slot = gif_dma_kick_plan.slot_for(i);
                let mut other = String::new();
                emit_gif_dma_capture(&mut other, &gif_dma_kick_plan, slot, "    ");

                if gif_dma_kick_plan.completes_at(i) {
                    let _ = writeln!(other, "    ctx->pc = 0x{:x}u;", inst.address);
                    let _ = writeln!(other, "    {}", gif_dma_kick_call(&gif_dma_kick_plan));
                    gif_dma_kick_plan = GifDmaKickPlan::default();
                }
                push_chunk(&mut chunks, ChunkKind::Other, other);

                update_constant_registers(inst, &mut constant_registers);
                i += 1;
                continue;
            }

            let mut statement = String::new();
            let memory_hint = resolve_memory_access_hint(inst, &constant_registers);
            let translation = cg.translate_instruction_with_hint(inst, memory_hint);
            match lean.and_then(|lean| lean_statement(inst, &translation, lean)) {
                Some(lean_text) => {
                    let _ = write!(statement, "    {lean_text}");
                }
                None => {
                    let _ = writeln!(statement, "    ctx->pc = 0x{:x}u;", inst.address);
                    let _ = write!(statement, "    {translation}");
                }
            }
            if inst.is_mmio {
                let _ = write!(statement, " // MMIO: 0x{:x}", inst.mmio_address);
            }
            statement.push('\n');
            if let Some(groups) = groups.as_mut() {
                groups.observe(inst, &translation, &mut chunks);
            }
            push_chunk(&mut chunks, ChunkKind::Instruction, statement);
            if let Some(groups) = groups.as_mut() {
                groups.settle(&translation, &mut chunks);
            }

            update_constant_registers(inst, &mut constant_registers);
        }

        i += 1;
    }

    if let Some(groups) = groups.as_mut() {
        groups.close_all(&mut chunks);
        // The groups are closed at every label a `goto` can reach: a goto into one would run members without their check.
        if let Some((target, first, last)) = groups.goto_into_a_group(&chunks) {
            panic!(
                "{}: goto label_{target:x} lands inside the RAM group 0x{first:x}..0x{last:x} (emit/ramgroup.rs)",
                function.name
            );
        }
        head.insert_str(0, groups.declarations());
    }

    // Fallthrough with no terminating branch: publish the next PC so the EE dispatcher does not re-enter this function.
    let mut tail = String::new();
    if !instructions.is_empty() && !last_instruction_was_control_flow {
        let _ = writeln!(tail, "    ctx->pc = 0x{:x}u;", function.end);
    }

    // GOW-Port (decision D2): `[codegen] register_locals`, unless the rewrite cannot place this function.
    let locals = cg.codegen().register_locals && locals_allowed(function.start);
    match locals.then(|| build_body(&chunks, &head, &tail)).flatten() {
        Some(body) => {
            // the locals macros (header guarded, so a unity batch of several such files includes it once)
            ss.insert_str(function_text_start, "#include \"ps2_cg.h\"\n");
            ss.push_str(&body);
        }
        None => {
            ss.push_str(&head);
            for chunk in &chunks {
                ss.push_str(&chunk.text);
            }
            ss.push_str(&tail);
        }
    }

    ss.push_str("}\n");
    Ok(ss)
}

/// GOW-Port: `PS2X_CODEGEN_FUNCS=<file>` (one hex start address per line, `#` comments) limits `register_locals` to those
/// functions, to bisect a difference while running the recompiler. Without it every function may use locals.
fn locals_allowed(start: u32) -> bool {
    use std::sync::OnceLock;
    static ONLY: OnceLock<Option<std::collections::HashSet<u32>>> = OnceLock::new();
    let only = ONLY.get_or_init(|| {
        let path = std::env::var_os("PS2X_CODEGEN_FUNCS")?;
        let text = std::fs::read_to_string(path).ok()?;
        Some(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .filter_map(|line| u32::from_str_radix(line.trim_start_matches("0x"), 16).ok())
                .collect(),
        )
    });
    only.as_ref().is_none_or(|set| set.contains(&start))
}
