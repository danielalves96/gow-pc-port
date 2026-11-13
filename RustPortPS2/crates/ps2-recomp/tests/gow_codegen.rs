//! The "gow" code generation switches seen through `emit_function`: lean pc for functions with a delay-slot entry and for
//! VU0 macro instructions, and the grouped RAM checks. The compat output must keep none of them.

use std::sync::Arc;

use ps2_config::Codegen;
use ps2_r5900::decode;
use ps2_recomp::emit::function::emit_function;
use ps2_recomp::generator::CodeGenerator;
use ps2_recomp::types::Function;

const BASE: u32 = 0x0010_0000;

fn emit(words: &[u32], codegen: Codegen) -> String {
    let instructions: Vec<_> =
        words.iter().enumerate().map(|(i, raw)| decode(BASE + 4 * i as u32, *raw, false)).collect();
    let function = Function {
        name: "sub_00100000".to_owned(),
        start: BASE,
        end: BASE + 4 * words.len() as u32,
        is_recompiled: true,
        ..Function::default()
    };
    let mut generator = CodeGenerator::new(&[], Arc::new(Vec::new()));
    generator.set_codegen(codegen);
    emit_function(&mut generator, &function, &instructions, false).unwrap()
}

fn lw(rt: u32, base: u32, offset: i16) -> u32 {
    (0x23 << 26) | (base << 21) | (rt << 16) | u32::from(offset as u16)
}

fn sw(rt: u32, base: u32, offset: i16) -> u32 {
    (0x2b << 26) | (base << 21) | (rt << 16) | u32::from(offset as u16)
}

const JR_RA: u32 = 0x03e0_0008;
const NOP: u32 = 0;

/// 0x100000 beq $a0, $zero -> 0x100010 (delay slot addiu $v0, $zero, 1 at 0x100004)
/// 0x100008 beq $a1, $zero -> 0x100004: a jump into the first branch's delay slot
/// 0x100010 jr $ra
const DELAY_SLOT_ENTRY: [u32; 7] = [0x1080_0003, 0x2402_0001, 0x10a0_fffe, NOP, 0x2403_0002, JR_RA, NOP];

#[test]
fn compat_keeps_the_delay_slot_test_and_every_pc_store() {
    let text = emit(&DELAY_SLOT_ENTRY, Codegen::default());
    assert!(text.contains("if (ctx->pc == 0x100004u) {"), "{text}");
    assert!(!text.contains("label_body_"), "{text}");
}

#[test]
fn a_delay_slot_entry_gets_its_own_label_and_the_function_stays_lean() {
    let text = emit(&DELAY_SLOT_ENTRY, Codegen::gow());
    assert!(!text.contains("ctx->pc == 0x100004u"), "the entry no longer tests the pc: {text}");
    assert!(text.contains("    goto label_body_100000;\nlabel_100004:\n    {\n"), "{text}");
    assert!(text.contains("label_body_100000:\n"), "{text}");
    // the entry block runs the slot and continues at the instruction after it
    let entry = &text[text.find("label_100004:").unwrap()..text.find("label_body_100000:").unwrap()];
    assert!(entry.contains("goto label_fallthrough_0x100000;"), "{entry}");
    assert!(text.contains("label_fallthrough_0x100000:\n"), "{text}");
    // lean: no delay-slot state around the slot, and no pc store before the second branch
    assert!(
        !text.contains("in_delay_slot")
            && !text.contains(
                "ctx->pc = 0x100008u;
        const bool"
            ),
        "{text}"
    );
    assert!(
        !text.contains(
            "ctx->pc = 0x100004u;
        ctx->in_delay_slot"
        ),
        "{text}"
    );

    let kept = emit(&DELAY_SLOT_ENTRY, Codegen { delay_slot_labels: false, ..Codegen::gow() });
    assert!(kept.contains("if (ctx->pc == 0x100004u) {"), "without the switch the old shape stays: {kept}");
}

#[test]
fn vu0_macro_instructions_drop_their_pc_store_only_with_lean_cop2() {
    // qmtc2.ni $a1, $vf1 ; vmulax.xyzw $ACC, $vf2, $vf1x ; jr $ra
    let words = [0x48a5_0800, 0x4be1_11bc, JR_RA, NOP];
    let lean = emit(&words, Codegen::gow());
    assert!(
        !lean.contains("ctx->pc = 0x100000u;\n    ctx->vu0_vf[1]") && !lean.contains("ctx->pc = 0x100004u;"),
        "{lean}"
    );
    assert!(lean.contains("ctx->vu0_vf[1] = "), "{lean}");
    let kept = emit(&words, Codegen { lean_cop2: false, ..Codegen::gow() });
    assert!(kept.contains("ctx->pc = 0x100004u;"), "{kept}");
}

#[test]
fn accesses_off_one_base_share_a_range_check() {
    // lw $v0, 0x10($s0) ; lw $v1, 0x14($s0) ; sw $v0, 0x20($s0) ; jr $ra
    let words = [lw(2, 16, 0x10), lw(3, 16, 0x14), sw(2, 16, 0x20), JR_RA, NOP];
    let text = emit(&words, Codegen::gow());
    assert!(text.contains("bool ps2g0f = false;"), "{text}");
    assert!(text.contains("ps2g0f = ps2g0m < (PS2_FAST_RAM_LIMIT - 16u);"), "{text}");
    assert!(text.contains("GREAD32(ps2g0f, ps2g0m, 0u, ADD32(GPR_U32(ctx, 16), 16))"), "{text}");
    assert!(text.contains("GWRITE32(ps2g0f, ps2g0m, 16u, ADD32(GPR_U32(ctx, 16), 32), GPR_U32(ctx, 2))"), "{text}");
    // the declarations come before the entry switch / first label: a goto may not jump over an initializer
    assert!(text.find("bool ps2g0f").unwrap() < text.find("label_100000:").unwrap_or(usize::MAX), "{text}");

    let off = emit(&words, Codegen { ram_groups: false, ..Codegen::gow() });
    assert!(!off.contains("ps2g0") && off.contains("READ32(ADD32(GPR_U32(ctx, 16), 16))"), "{off}");
    let compat = emit(&words, Codegen::default());
    assert!(!compat.contains("ps2g0") && !compat.contains("GREAD"), "{compat}");
}

#[test]
fn a_label_a_goto_reaches_ends_the_group() {
    // 0x100000 lw $v0, 0($s0)
    // 0x100004 lw $v1, 4($s0)      <- bne below jumps here
    // 0x100008 lw $a0, 8($s0)
    // 0x10000c bne $v0, $zero -> 0x100004 (delay slot nop)
    // 0x100014 jr $ra
    let words = [lw(2, 16, 0), lw(3, 16, 4), lw(4, 16, 8), 0x1440_fffd, NOP, JR_RA, NOP];
    let text = emit(&words, Codegen::gow());
    // 0x100000 stands alone (no group), 0x100004 and 0x100008 share one
    assert!(!text.contains("GREAD32(ps2g0f, ps2g0m, 0u, ADD32(GPR_U32(ctx, 16), 0))"), "{text}");
    assert!(text.contains("GREAD32(ps2g0f, ps2g0m, 0u, ADD32(GPR_U32(ctx, 16), 4))"), "{text}");
    assert!(text.contains("GREAD32(ps2g0f, ps2g0m, 4u, ADD32(GPR_U32(ctx, 16), 8))"), "{text}");
    let label = text.find("label_100004:").unwrap();
    let setup = text.find("ps2g0f = ps2g0m <").unwrap();
    assert!(label < setup, "the check sits after the label a loop jumps to: {text}");
}

#[test]
fn a_write_to_the_base_register_ends_the_group() {
    // lw $s0, 0($s0) ; lw $v1, 4($s0) ; lw $v0, 8($s0) ; jr $ra: the second and third use the new $s0
    let words = [lw(16, 16, 0), lw(3, 16, 4), lw(2, 16, 8), JR_RA, NOP];
    let text = emit(&words, Codegen::gow());
    assert!(text.contains("GREAD32(ps2g0f, ps2g0m, 0u, ADD32(GPR_U32(ctx, 16), 4))"), "{text}");
    assert!(!text.contains("ps2g1"), "{text}");
    assert!(
        text.contains("SET_GPR_S32(ctx, 16, (int32_t)READ32(ADD32(GPR_U32(ctx, 16), 0)))"),
        "the loader stays alone: {text}"
    );
}
