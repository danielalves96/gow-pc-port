//! The VU0 macro block of `COP2`.
//!
//! Port of the `COP2_CO` dispatcher in `vu_translator.cpp` and of every
//! `translateVU_*` function in `vu_translation_helpers.cpp`. The generated text is
//! compared byte for byte, so each template is the C++ literal with its
//! `{}` placeholders turned into named arguments; the adjacent C++ literals are
//! joined with `concat!`.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::codegen_helpers::{format_float_literal, vu_mask_expr};
use crate::translate::instruction::FamilyTranslator;

/// The four `(dest_mask & bit) ? -1 : 0` arguments of `_mm_set_epi32`, as text.
fn mask_args(dest_mask: u8) -> String {
    let lane = |bit: u8| if dest_mask & bit != 0 { -1 } else { 0 };
    format!("{}, {}, {}, {}", lane(0x1), lane(0x2), lane(0x4), lane(0x8))
}

/// `_MM_SHUFFLE(f,f,f,f)` for the broadcast field of the instruction.
fn broadcast_shuffle(inst: &Instruction) -> String {
    let field = inst.function & 0x3;
    format!("_MM_SHUFFLE({field},{field},{field},{field})")
}

fn dest_mask(inst: &Instruction) -> u8 {
    inst.vector.vector_field
}

/// `{ __m128 res = <op>(vf[vfs], <broadcast of vf[vft]>); mask...; vf[vfd] = blendv }`
fn field_op(inst: &Instruction, op: &str) -> String {
    let (vfd, vfs, vft) = (inst.sa, inst.rd, inst.rt);
    let shuffle = broadcast_shuffle(inst);
    let mask = mask_args(dest_mask(inst));
    format!(
        "{{ __m128 res = {op}(ctx->vu0_vf[{vfs}], _mm_shuffle_ps(ctx->vu0_vf[{vft}], ctx->vu0_vf[{vft}], {shuffle})); __m128i mask = _mm_set_epi32({mask}); ctx->vu0_vf[{vfd}] = _mm_blendv_ps(ctx->vu0_vf[{vfd}], res, _mm_castsi128_ps(mask)); }}"
    )
}

/// `{ __m128 res = <op>(vf[vfs], vf[vft]); mask; vf[vfd] = PS2_VBLEND(...) }`
fn vector_op(inst: &Instruction, op: &str) -> String {
    let (vfd, vfs, vft) = (inst.sa, inst.rd, inst.rt);
    let mask = mask_args(dest_mask(inst));
    format!(
        "{{ __m128 res = {op}(ctx->vu0_vf[{vfs}], ctx->vu0_vf[{vft}]); __m128i mask = _mm_set_epi32({mask}); ctx->vu0_vf[{vfd}] = PS2_VBLEND(ctx->vu0_vf[{vfd}], res, _mm_castsi128_ps(mask)); }}"
    )
}

/// `{ __m128 res = <op>(vf[vfs], <scalar>); mask; vf[vfd] = _mm_blendv_ps(...) }` with `scalar`
/// being `_mm_set1_ps(ctx->vu0_q)` or `_mm_set1_ps(ctx->vu0_i)`.
fn scalar_op(inst: &Instruction, op: &str, scalar: &str) -> String {
    let (vfd, vfs) = (inst.sa, inst.rd);
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128 res = {op}(ctx->vu0_vf[{vfs}], _mm_set1_ps(ctx->vu0_{scalar})); ",
            "__m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{vfd}] = _mm_blendv_ps(ctx->vu0_vf[{vfd}], res, _mm_castsi128_ps(mask)); }}"
        ),
        op = op,
        vfs = vfs,
        scalar = scalar,
        mask = mask,
        vfd = vfd
    )
}

/// The `res = <op>(vf[vfs], vf[vft])` forms of min/max (no PS2_ wrapper).
fn plain_vector_op(inst: &Instruction, op: &str) -> String {
    let (vfd, vfs, vft) = (inst.sa, inst.rd, inst.rt);
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128 res = {op}(ctx->vu0_vf[{vfs}], ctx->vu0_vf[{vft}]); ",
            "__m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{vfd}] = _mm_blendv_ps(ctx->vu0_vf[{vfd}], res, _mm_castsi128_ps(mask)); }}"
        ),
        op = op,
        vfs = vfs,
        vft = vft,
        mask = mask,
        vfd = vfd
    )
}

/// `translateVU_VMADD_Field` / `VMSUB_Field`: `acc <op> (vf[vfs] * broadcast(vf[vft]))`.
fn multiply_accumulate_field(inst: &Instruction, acc_op: &str) -> String {
    let (vfd, vfs, vft) = (inst.sa, inst.rd, inst.rt);
    let shuffle = broadcast_shuffle(inst);
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128 mul_res = PS2_VMUL(ctx->vu0_vf[{vfs}], _mm_shuffle_ps(ctx->vu0_vf[{vft}], ctx->vu0_vf[{vft}], {shuffle})); ",
            "__m128 res = {acc_op}(ctx->vu0_acc, mul_res); ",
            "__m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{vfd}] = _mm_blendv_ps(ctx->vu0_vf[{vfd}], res, _mm_castsi128_ps(mask)); }}"
        ),
        vfs = vfs,
        vft = vft,
        shuffle = shuffle,
        acc_op = acc_op,
        mask = mask,
        vfd = vfd
    )
}

/// `translateVU_VMADD` / `VMSUB`: full-vector multiply-accumulate.
fn multiply_accumulate_vector(inst: &Instruction, acc_op: &str) -> String {
    let (vfd, vfs, vft) = (inst.sa, inst.rd, inst.rt);
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128 mul_res = PS2_VMUL(ctx->vu0_vf[{vfs}], ctx->vu0_vf[{vft}]); ",
            "__m128 res = {acc_op}(ctx->vu0_acc, mul_res); ",
            "__m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{vfd}] = _mm_blendv_ps(ctx->vu0_vf[{vfd}], res, _mm_castsi128_ps(mask)); }}"
        ),
        vfs = vfs,
        vft = vft,
        acc_op = acc_op,
        mask = mask,
        vfd = vfd
    )
}

/// `translateVU_VMADDq/i` / `VMSUBq/i`: multiply-accumulate with Q or I.
fn multiply_accumulate_scalar(inst: &Instruction, acc_op: &str, scalar: &str) -> String {
    let (vfd, vfs) = (inst.sa, inst.rd);
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128 mul_res = PS2_VMUL(ctx->vu0_vf[{vfs}], _mm_set1_ps(ctx->vu0_{scalar})); ",
            "__m128 res = {acc_op}(ctx->vu0_acc, mul_res); ",
            "__m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{vfd}] = _mm_blendv_ps(ctx->vu0_vf[{vfd}], res, _mm_castsi128_ps(mask)); }}"
        ),
        vfs = vfs,
        scalar = scalar,
        acc_op = acc_op,
        mask = mask,
        vfd = vfd
    )
}

/// `translateVU_V{ADD,SUB,MUL}A_Field`: the accumulator forms with a broadcast field.
fn acc_field_op(inst: &Instruction, op: &str) -> String {
    let (vfs, vft) = (inst.rd, inst.rt);
    let shuffle = broadcast_shuffle(inst);
    let mask = vu_mask_expr(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128 res = {op}(ctx->vu0_vf[{vfs}], _mm_shuffle_ps(ctx->vu0_vf[{vft}], ctx->vu0_vf[{vft}], {shuffle})); ",
            "ctx->vu0_acc = _mm_blendv_ps(ctx->vu0_acc, res, {mask}); }}"
        ),
        op = op,
        vfs = vfs,
        vft = vft,
        shuffle = shuffle,
        mask = mask
    )
}

/// `translateVU_VMADDA_Field` / `VMSUBA_Field`.
fn acc_multiply_field(inst: &Instruction, acc_op: &str) -> String {
    let (vfs, vft) = (inst.rd, inst.rt);
    let shuffle = broadcast_shuffle(inst);
    let mask = vu_mask_expr(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128 mul_res = PS2_VMUL(ctx->vu0_vf[{vfs}], _mm_shuffle_ps(ctx->vu0_vf[{vft}], ctx->vu0_vf[{vft}], {shuffle})); ",
            "__m128 res = {acc_op}(ctx->vu0_acc, mul_res); ",
            "ctx->vu0_acc = _mm_blendv_ps(ctx->vu0_acc, res, {mask}); }}"
        ),
        vfs = vfs,
        vft = vft,
        shuffle = shuffle,
        acc_op = acc_op,
        mask = mask
    )
}

/// `translateVU_V{ADD,SUB,MUL}A`: `acc = op(vf[vfs], vf[vft])`.
fn acc_vector_op(inst: &Instruction, op: &str) -> String {
    let (vfs, vft) = (inst.rd, inst.rt);
    let mask = vu_mask_expr(dest_mask(inst));
    format!(
        "{{ __m128 res = {op}(ctx->vu0_vf[{vfs}], ctx->vu0_vf[{vft}]); ctx->vu0_acc = _mm_blendv_ps(ctx->vu0_acc, res, {mask}); }}"
    )
}

/// `translateVU_V{ADD,SUB,MUL}A{q,i}`: `acc = op(vf[vfs], Q or I)`.
fn acc_scalar_op(inst: &Instruction, op: &str, scalar: &str) -> String {
    let vfs = inst.rd;
    let mask = vu_mask_expr(dest_mask(inst));
    format!(
        "{{ __m128 res = {op}(ctx->vu0_vf[{vfs}], _mm_set1_ps(ctx->vu0_{scalar})); ctx->vu0_acc = _mm_blendv_ps(ctx->vu0_acc, res, {mask}); }}"
    )
}

/// `translateVU_VMADDA` / `VMSUBA`.
fn acc_multiply_vector(inst: &Instruction, acc_op: &str) -> String {
    let (vfs, vft) = (inst.rd, inst.rt);
    let mask = vu_mask_expr(dest_mask(inst));
    format!(
        "{{ __m128 mul_res = PS2_VMUL(ctx->vu0_vf[{vfs}], ctx->vu0_vf[{vft}]); __m128 res = {acc_op}(ctx->vu0_acc, mul_res); ctx->vu0_acc = _mm_blendv_ps(ctx->vu0_acc, res, {mask}); }}"
    )
}

/// `translateVU_VMADDAq/i` / `VMSUBAq/i`.
fn acc_multiply_scalar(inst: &Instruction, acc_op: &str, scalar: &str) -> String {
    let vfs = inst.rd;
    let mask = vu_mask_expr(dest_mask(inst));
    format!(
        "{{ __m128 mul_res = PS2_VMUL(ctx->vu0_vf[{vfs}], _mm_set1_ps(ctx->vu0_{scalar})); __m128 res = {acc_op}(ctx->vu0_acc, mul_res); ctx->vu0_acc = _mm_blendv_ps(ctx->vu0_acc, res, {mask}); }}"
    )
}

/// `translateVU_VDIV` / `VRSQRT`: `Q = fn(status, vf[fs].fsf, vf[ft].ftf)`.
fn quotient_op(inst: &Instruction, function: &str) -> String {
    let (fsf, ftf) = (inst.vector.fsf, inst.vector.ftf);
    let (fs_reg, ft_reg) = (inst.rd, inst.rt);
    format!(
        concat!(
            "{{ float fs = _mm_cvtss_f32(_mm_shuffle_ps(ctx->vu0_vf[{fs_reg}], ctx->vu0_vf[{fs_reg}], _MM_SHUFFLE(0,0,0,{fsf}))); ",
            "float ft = _mm_cvtss_f32(_mm_shuffle_ps(ctx->vu0_vf[{ft_reg}], ctx->vu0_vf[{ft_reg}], _MM_SHUFFLE(0,0,0,{ftf}))); ",
            "ctx->vu0_q = {function}(ctx->vu0_status, fs, ft); }}"
        ),
        fs_reg = fs_reg,
        fsf = fsf,
        ft_reg = ft_reg,
        ftf = ftf,
        function = function
    )
}

fn vsqrt(inst: &Instruction) -> String {
    let ftf = inst.vector.ftf;
    let ft_reg = inst.rt;
    format!(
        "{{ float ft = _mm_cvtss_f32(_mm_shuffle_ps(ctx->vu0_vf[{ft_reg}], ctx->vu0_vf[{ft_reg}], _MM_SHUFFLE(0,0,0,{ftf}))); ctx->vu0_q = ps2VuSqrt(ctx->vu0_status, ft); }}"
    )
}

fn vmtir(inst: &Instruction) -> String {
    let fsf = inst.vector.fsf;
    format!(
        "{{ uint32_t bits; float src = _mm_cvtss_f32(_mm_shuffle_ps(ctx->vu0_vf[{rd}], ctx->vu0_vf[{rd}], _MM_SHUFFLE(0,0,0,{fsf}))); std::memcpy(&bits, &src, sizeof(bits)); ctx->vi[{rt}] = (uint16_t)(bits & 0xFFFF); }}",
        rd = inst.rd,
        rt = inst.rt
    )
}

fn vmfir(inst: &Instruction) -> String {
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ uint32_t tmp = (uint32_t)(int32_t)(int16_t)ctx->vi[{rd}]; float val; std::memcpy(&val, &tmp, sizeof(val)); ",
            "__m128 res = _mm_set1_ps(val); ",
            "__m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{rt}] = _mm_blendv_ps(ctx->vu0_vf[{rt}], res, _mm_castsi128_ps(mask)); }}"
        ),
        rd = inst.rd,
        mask = mask,
        rt = inst.rt
    )
}

fn vilwr(inst: &Instruction) -> String {
    let mask = dest_mask(inst);
    if inst.rt == 0 {
        return "{ }".to_owned();
    }
    let lane: u32 = if mask & 0x8 != 0 {
        0
    } else if mask & 0x4 != 0 {
        1
    } else if mask & 0x2 != 0 {
        2
    } else {
        3
    };
    format!(
        "{{ uint16_t value; std::memcpy(&value, runtime->memory().getVU0Data() + ((static_cast<uint32_t>(ctx->vi[{}]) & 0xFFu) << 4) + {}u, sizeof(value)); ctx->vi[{}] = value; }}",
        inst.rd,
        lane * 4,
        inst.rt
    )
}

fn viswr(inst: &Instruction) -> String {
    let mask = dest_mask(inst);
    let mut code = format!(
        "{{ uint8_t *vu0Mem = runtime->memory().getVU0Data() + ((static_cast<uint32_t>(ctx->vi[{}]) & 0xFFu) << 4); const uint32_t value = ctx->vi[{}]; ",
        inst.rd, inst.rt
    );
    for lane in 0..4u32 {
        if mask & (0x8 >> lane) != 0 {
            code.push_str(&format!("std::memcpy(vu0Mem + {}u, &value, sizeof(value)); ", lane * 4));
        }
    }
    code.push('}');
    code
}

/// `VIADD`/`VISUB`/`VIAND`/`VIOR`: `vi[sa] = vi[rd] <op> vi[rt]`.
fn integer_op(inst: &Instruction, wrap: bool, op: char) -> String {
    if inst.sa == 0 {
        return "{ }".to_owned();
    }
    if wrap {
        format!("ctx->vi[{}] = static_cast<uint16_t>(ctx->vi[{}] {op} ctx->vi[{}]);", inst.sa, inst.rd, inst.rt)
    } else {
        format!("ctx->vi[{}] = ctx->vi[{}] {op} ctx->vi[{}];", inst.sa, inst.rd, inst.rt)
    }
}

fn viaddi(inst: &Instruction) -> String {
    let imm5: i32 = if inst.sa & 0x10 != 0 { (inst.sa | !0x1F) as i32 } else { inst.sa as i32 };
    if inst.rt == 0 {
        return "{ }".to_owned();
    }
    format!("ctx->vi[{}] = static_cast<uint16_t>(ctx->vi[{}] + {});", inst.rt, inst.rd, imm5)
}

/// `VCALLMS` calls a VU0 microprogram at the immediate instruction index (0-511, 8 bytes each).
fn vcallms(inst: &Instruction) -> String {
    let instr_index = (inst.raw >> 6) & 0x1FF;
    let target_byte_addr = instr_index << 3;
    format!(
        concat!(
            "{{ ",
            "    ctx->vu0_tpc = 0x{target:X}; ",
            "    runtime->executeVU0Microprogram(rdram, ctx, 0x{target:X}); ",
            "}}"
        ),
        target = target_byte_addr
    )
}

/// `VCALLMSR` calls a VU0 microprogram at the address held in CMSAR0.
///
/// GOW-Port: `[codegen] vu0_macro_fixes`. PS2Recomp's text reads `ctx->vi[rd]`, and `rd` is 27 here (the CMSAR0 control
/// register): past the 16-entry `vi` array, so the program started at whatever followed it (gta-vcs-recomp,
/// docs/PS2RECOMP_PATCHES.md). Without the switch the text stays PS2Recomp's, for byte parity.
fn vcallmsr(inst: &Instruction, fixed: bool) -> String {
    let start = if fixed { "ctx->vu0_cmsar0".to_owned() } else { format!("ctx->vi[{}]", inst.rd) };
    format!(
        concat!(
            "{{ ",
            "    uint16_t instr_index = {start} & 0x1FF; ",
            "    uint32_t target_byte_addr = (uint32_t)instr_index << 3; ",
            "    ctx->vu0_pc = target_byte_addr; ",
            "    runtime->vu0StartMicroProgram(rdram, ctx, target_byte_addr); ",
            "}}"
        ),
        start = start
    )
}

/// GOW-Port: VRNEXT/VRINIT/VRXOR/VRGET follow the hardware R register (LFSR, SotC fork, e36fbf1).
fn vrnext(inst: &Instruction) -> String {
    let mut code = String::from(
        "{ uint32_t r = static_cast<uint32_t>(_mm_cvtsi128_si32(_mm_castps_si128(ctx->vu0_r))); \
         r = (((r << 1) ^ ((r >> 4) & 1u) ^ ((r >> 22) & 1u)) & 0x007FFFFFu) | 0x3F800000u; \
         ctx->vu0_r = _mm_castsi128_ps(_mm_set1_epi32(static_cast<int32_t>(r))); ",
    );
    if inst.rt != 0 {
        code.push_str(&format!(
            "ctx->vu0_vf[{rt}] = _mm_blendv_ps(ctx->vu0_vf[{rt}], ctx->vu0_r, {mask}); ",
            rt = inst.rt,
            mask = vu_mask_expr(dest_mask(inst))
        ));
    }
    code.push('}');
    code
}

fn vrget(inst: &Instruction) -> String {
    if inst.rt == 0 {
        return "{ }".to_owned();
    }
    format!(
        concat!(
            "{{ const __m128 res = _mm_castsi128_ps(_mm_shuffle_epi32(_mm_castps_si128(ctx->vu0_r), 0)); ",
            "ctx->vu0_vf[{rt}] = _mm_blendv_ps(ctx->vu0_vf[{rt}], res, {mask}); }}"
        ),
        rt = inst.rt,
        mask = vu_mask_expr(dest_mask(inst))
    )
}

fn vrinit(inst: &Instruction) -> String {
    format!(
        concat!(
            "{{ alignas(16) uint32_t fs[4]; _mm_store_si128(reinterpret_cast<__m128i *>(fs), _mm_castps_si128(ctx->vu0_vf[{rd}])); ",
            "ctx->vu0_r = _mm_castsi128_ps(_mm_set1_epi32(static_cast<int32_t>(0x3F800000u | (fs[{fsf}] & 0x007FFFFFu)))); }}"
        ),
        rd = inst.rd,
        fsf = inst.vector.fsf
    )
}

fn vrxor(inst: &Instruction) -> String {
    format!(
        concat!(
            "{{ alignas(16) uint32_t fs[4]; _mm_store_si128(reinterpret_cast<__m128i *>(fs), _mm_castps_si128(ctx->vu0_vf[{rd}])); ",
            "const uint32_t r = static_cast<uint32_t>(_mm_cvtsi128_si32(_mm_castps_si128(ctx->vu0_r))); ",
            "ctx->vu0_r = _mm_castsi128_ps(_mm_set1_epi32(static_cast<int32_t>(0x3F800000u | ((r ^ fs[{fsf}]) & 0x007FFFFFu)))); }}"
        ),
        rd = inst.rd,
        fsf = inst.vector.fsf
    )
}

/// `translateVU_VOPMSUB`: cross product subtracted from the accumulator.
fn vopmsub(inst: &Instruction) -> String {
    let (vfd, vfs, vft) = (inst.sa, inst.rd, inst.rt);
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128 fs_yzx = _mm_shuffle_ps(ctx->vu0_vf[{vfs}], ctx->vu0_vf[{vfs}], _MM_SHUFFLE(3,0,2,1)); ",
            "__m128 ft_zxy = _mm_shuffle_ps(ctx->vu0_vf[{vft}], ctx->vu0_vf[{vft}], _MM_SHUFFLE(3,1,0,2)); ",
            "__m128 mul_res = PS2_VMUL(fs_yzx, ft_zxy); ",
            "__m128 res = PS2_VSUB(ctx->vu0_acc, mul_res); ",
            "__m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{vfd}] = _mm_blendv_ps(ctx->vu0_vf[{vfd}], res, _mm_castsi128_ps(mask)); }}"
        ),
        vfs = vfs,
        vft = vft,
        mask = mask,
        vfd = vfd
    )
}

/// `translateVU_VOPMULA`: cross product into the accumulator.
fn vopmula(inst: &Instruction) -> String {
    let (vfs, vft) = (inst.rd, inst.rt);
    let mask = vu_mask_expr(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128 fs_yzx = _mm_shuffle_ps(ctx->vu0_vf[{vfs}], ctx->vu0_vf[{vfs}], _MM_SHUFFLE(3,0,2,1)); ",
            "__m128 ft_zxy = _mm_shuffle_ps(ctx->vu0_vf[{vft}], ctx->vu0_vf[{vft}], _MM_SHUFFLE(3,1,0,2)); ",
            "__m128 res = PS2_VMUL(fs_yzx, ft_zxy); ",
            "ctx->vu0_acc = _mm_blendv_ps(ctx->vu0_acc, res, {mask}); }}"
        ),
        vfs = vfs,
        vft = vft,
        mask = mask
    )
}

fn vitof(inst: &Instruction, shift: u32) -> String {
    let vfs = inst.rd;
    let scale: f32 = if shift == 0 { 1.0 } else { 1.0 / ((1u32 << shift) as f32) };
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128i src = _mm_castps_si128(ctx->vu0_vf[{vfs}]); ",
            "__m128 res = _mm_cvtepi32_ps(src); ",
            "res = _mm_mul_ps(res, _mm_set1_ps({scale})); ",
            "__m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{rt}] = _mm_blendv_ps(ctx->vu0_vf[{rt}], res, _mm_castsi128_ps(mask)); }}"
        ),
        vfs = vfs,
        scale = format_float_literal(scale),
        mask = mask,
        rt = inst.rt
    )
}

fn vftoi(inst: &Instruction, shift: u32) -> String {
    if inst.rt == 0 {
        return "{ }".to_owned();
    }
    let scale: f32 = if shift == 0 { 1.0 } else { (1u32 << shift) as f32 };
    format!(
        concat!(
            "{{ const __m128 src = _mm_mul_ps(ctx->vu0_vf[{rd}], _mm_set1_ps({scale})); ",
            "const __m128i positiveOverflow = _mm_andnot_si128(_mm_srai_epi32(_mm_castps_si128(src), 31), _mm_castps_si128(_mm_cmpnlt_ps(src, _mm_set1_ps(2147483648.0f)))); ",
            "const __m128 res = _mm_castsi128_ps(_mm_xor_si128(_mm_cvttps_epi32(src), positiveOverflow)); ",
            "ctx->vu0_vf[{rt}] = _mm_blendv_ps(ctx->vu0_vf[{rt}], res, {mask}); }}"
        ),
        rd = inst.rd,
        scale = format_float_literal(scale),
        rt = inst.rt,
        mask = vu_mask_expr(dest_mask(inst))
    )
}

/// GOW-Port: VLQI/VSQI/VLQD/VSQD/VILWR/VISWR use VU0's data memory ((vi & 0xFF) << 4), not the EE's RAM
/// from address 0; VSQI/VSQD store Fs at the address of It (SotC fork, 03d18df).
fn vu0_memory_load(ft: u32, is: u32, mask: u8, pre_decrement: bool, post_increment: bool) -> String {
    let mut code = String::from("{ ");
    if pre_decrement && is != 0 {
        code.push_str(&format!("ctx->vi[{is}] = static_cast<uint16_t>(ctx->vi[{is}] - 1u); "));
    }
    if ft != 0 {
        code.push_str(&format!(
            "__m128 res; std::memcpy(&res, runtime->memory().getVU0Data() + ((static_cast<uint32_t>(ctx->vi[{is}]) & 0xFFu) << 4), sizeof(res)); ctx->vu0_vf[{ft}] = _mm_blendv_ps(ctx->vu0_vf[{ft}], res, {}); ",
            vu_mask_expr(mask)
        ));
    }
    if post_increment && is != 0 {
        code.push_str(&format!("ctx->vi[{is}] = static_cast<uint16_t>(ctx->vi[{is}] + 1u); "));
    }
    code.push('}');
    code
}

fn vu0_memory_store(fs: u32, it: u32, mask: u8, pre_decrement: bool, post_increment: bool) -> String {
    let mut code = String::from("{ ");
    if pre_decrement && it != 0 {
        code.push_str(&format!("ctx->vi[{it}] = static_cast<uint16_t>(ctx->vi[{it}] - 1u); "));
    }
    code.push_str(&format!(
        "uint8_t *vu0Mem = runtime->memory().getVU0Data() + ((static_cast<uint32_t>(ctx->vi[{it}]) & 0xFFu) << 4); __m128 old; std::memcpy(&old, vu0Mem, sizeof(old)); const __m128 res = _mm_blendv_ps(old, ctx->vu0_vf[{fs}], {}); std::memcpy(vu0Mem, &res, sizeof(res)); ",
        vu_mask_expr(mask)
    ));
    if post_increment && it != 0 {
        code.push_str(&format!("ctx->vi[{it}] = static_cast<uint16_t>(ctx->vi[{it}] + 1u); "));
    }
    code.push('}');
    code
}

/// The `VABS`, `VMOVE`, `VMR32` and `VCLIPw` texts that `vu_translator.cpp` builds itself.
fn vabs(inst: &Instruction) -> String {
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ const __m128i bits = _mm_castps_si128(ctx->vu0_vf[{rd}]); ",
            "const __m128i denormal = _mm_cmpeq_epi32(_mm_and_si128(bits, _mm_set1_epi32(0x7F800000)), _mm_setzero_si128()); ",
            "__m128 res = _mm_castsi128_ps(_mm_andnot_si128(denormal, _mm_and_si128(bits, _mm_set1_epi32(0x7FFFFFFF)))); ",
            "__m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{rt}] = _mm_blendv_ps(ctx->vu0_vf[{rt}], res, _mm_castsi128_ps(mask)); }}"
        ),
        rd = inst.rd,
        mask = mask,
        rt = inst.rt
    )
}

fn vmove(inst: &Instruction) -> String {
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{rt}] = _mm_blendv_ps(ctx->vu0_vf[{rt}], ctx->vu0_vf[{rd}], _mm_castsi128_ps(mask)); }}"
        ),
        mask = mask,
        rt = inst.rt,
        rd = inst.rd
    )
}

fn vmr32(inst: &Instruction) -> String {
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128 res = _mm_shuffle_ps(ctx->vu0_vf[{rd}], ctx->vu0_vf[{rd}], _MM_SHUFFLE(0,3,2,1)); ",
            "__m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{rt}] = _mm_blendv_ps(ctx->vu0_vf[{rt}], res, _mm_castsi128_ps(mask)); }}"
        ),
        rd = inst.rd,
        mask = mask,
        rt = inst.rt
    )
}

/// GOW-Port: with `[codegen] vu0_macro_fixes`, CLIP sets bit 0 when x > +|w| and bit 1 when x < -|w|, then y (bits 2-3)
/// and z (bits 4-5), as the VU1 interpreter does. PS2Recomp's text (kept without the switch, for byte parity) swaps the +
/// and - bits and compares with w instead of |w| (gta-vcs-recomp, docs/PS2RECOMP_PATCHES.md).
fn vclipw(inst: &Instruction, fixed: bool) -> String {
    let field = inst.function & 0x3;
    let shuffle_pattern = format!("_MM_SHUFFLE({field},{field},{field},{field})");
    let abs = if fixed { "ft = _mm_and_ps(ft, _mm_castsi128_ps(_mm_set1_epi32(0x7FFFFFFF))); " } else { "" };
    let (plus, minus) = if fixed { ("gt", "lt") } else { ("lt", "gt") };
    format!(
        concat!(
            "{{ __m128 fs = ctx->vu0_vf[{rd}]; ",
            "__m128 ft = _mm_shuffle_ps(ctx->vu0_vf[{rt}], ctx->vu0_vf[{rt}], {shuffle}); ",
            "{abs}",
            "__m128 neg_ft = _mm_xor_ps(ft, _mm_castsi128_ps(_mm_set1_epi32(0x80000000))); ",
            "__m128 gt = _mm_cmpgt_ps(fs, ft); ",
            "__m128 lt = _mm_cmplt_ps(fs, neg_ft); ",
            "uint32_t gt_mask = (uint32_t)_mm_movemask_ps(gt); ",
            "uint32_t lt_mask = (uint32_t)_mm_movemask_ps(lt); ",
            "uint32_t flags = (({p}_mask & 0x1) << 0) | (({m}_mask & 0x1) << 1) | ",
            "(({p}_mask & 0x2) << 1) | (({m}_mask & 0x2) << 2) | ",
            "(({p}_mask & 0x4) << 2) | (({m}_mask & 0x4) << 3); ",
            "ctx->vu0_clip_flags = ((ctx->vu0_clip_flags << 6) | (flags & 0x3F)) & 0xFFFFFF; }}"
        ),
        abs = abs,
        p = plus,
        m = minus,
        rd = inst.rd,
        rt = inst.rt,
        shuffle = shuffle_pattern
    )
}

/// The `COP2_CO` block: the Special1 (function) and Special2 (`sa:function[1:0]`) tables.
#[allow(non_upper_case_globals)] // the opcode constants keep the C++ names (`VU0_S2_VADDAx`)
pub fn translate_macro(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    let special1_func = inst.function & 0x3F;
    if special1_func >= 0x3C {
        let vu_func = (((inst.raw >> 6) & 0x1F) << 2) | (inst.raw & 0x3);
        return match vu_func {
            VU0_S2_VADDAx | VU0_S2_VADDAy | VU0_S2_VADDAz | VU0_S2_VADDAw => acc_field_op(inst, "PS2_VADD"),
            VU0_S2_VSUBAx | VU0_S2_VSUBAy | VU0_S2_VSUBAz | VU0_S2_VSUBAw => acc_field_op(inst, "PS2_VSUB"),
            VU0_S2_VMADDAx | VU0_S2_VMADDAy | VU0_S2_VMADDAz | VU0_S2_VMADDAw => acc_multiply_field(inst, "PS2_VADD"),
            VU0_S2_VMSUBAx | VU0_S2_VMSUBAy | VU0_S2_VMSUBAz | VU0_S2_VMSUBAw => acc_multiply_field(inst, "PS2_VSUB"),
            VU0_S2_VMULAx | VU0_S2_VMULAy | VU0_S2_VMULAz | VU0_S2_VMULAw => acc_field_op(inst, "PS2_VMUL"),
            VU0_S2_VADDA => acc_vector_op(inst, "PS2_VADD"),
            VU0_S2_VADDAq => acc_scalar_op(inst, "PS2_VADD", "q"),
            VU0_S2_VADDAi => acc_scalar_op(inst, "PS2_VADD", "i"),
            VU0_S2_VMADDA => acc_multiply_vector(inst, "PS2_VADD"),
            VU0_S2_VMADDAq => acc_multiply_scalar(inst, "PS2_VADD", "q"),
            VU0_S2_VMADDAi => acc_multiply_scalar(inst, "PS2_VADD", "i"),
            VU0_S2_VSUBA => acc_vector_op(inst, "PS2_VSUB"),
            VU0_S2_VSUBAq => acc_scalar_op(inst, "PS2_VSUB", "q"),
            VU0_S2_VSUBAi => acc_scalar_op(inst, "PS2_VSUB", "i"),
            VU0_S2_VMSUBA => acc_multiply_vector(inst, "PS2_VSUB"),
            VU0_S2_VMSUBAq => acc_multiply_scalar(inst, "PS2_VSUB", "q"),
            VU0_S2_VMSUBAi => acc_multiply_scalar(inst, "PS2_VSUB", "i"),
            VU0_S2_VMULA => acc_vector_op(inst, "PS2_VMUL"),
            VU0_S2_VMULAq => acc_scalar_op(inst, "PS2_VMUL", "q"),
            VU0_S2_VMULAi => acc_scalar_op(inst, "PS2_VMUL", "i"),
            VU0_S2_VOPMULA => vopmula(inst),
            VU0_S2_VITOF0 => vitof(inst, 0),
            VU0_S2_VITOF4 => vitof(inst, 4),
            VU0_S2_VITOF12 => vitof(inst, 12),
            VU0_S2_VITOF15 => vitof(inst, 15),
            VU0_S2_VFTOI0 => vftoi(inst, 0),
            VU0_S2_VFTOI4 => vftoi(inst, 4),
            VU0_S2_VFTOI12 => vftoi(inst, 12),
            VU0_S2_VFTOI15 => vftoi(inst, 15),
            VU0_S2_VLQI => vu0_memory_load(inst.rt, inst.rd, dest_mask(inst), false, true),
            VU0_S2_VSQI => vu0_memory_store(inst.rd, inst.rt, dest_mask(inst), false, true),
            VU0_S2_VLQD => vu0_memory_load(inst.rt, inst.rd, dest_mask(inst), true, false),
            VU0_S2_VSQD => vu0_memory_store(inst.rd, inst.rt, dest_mask(inst), true, false),
            VU0_S2_VDIV => quotient_op(inst, "ps2VuDivide"),
            VU0_S2_VSQRT => vsqrt(inst),
            // GOW-Port: VDIV/VSQRT/VRSQRT with the PS2's semantics (no infinities; VRSQRT is FS/sqrt(|FT|)).
            VU0_S2_VRSQRT => quotient_op(inst, "ps2VuRsqrt"),
            VU0_S2_VWAITQ => "// VWAITQ (Q already resolved in this runtime)".to_owned(),
            VU0_S2_VMTIR => vmtir(inst),
            VU0_S2_VMFIR => vmfir(inst),
            VU0_S2_VILWR => vilwr(inst),
            VU0_S2_VISWR => viswr(inst),
            VU0_S2_VABS => vabs(inst),
            VU0_S2_VMOVE => vmove(inst),
            VU0_S2_VMR32 => vmr32(inst),
            VU0_S2_VCLIPw => vclipw(inst, hooks.codegen().vu0_macro_fixes),
            VU0_S2_VNOP => "// NOP operation, no action needed for VU0".to_owned(),
            VU0_S2_VRNEXT => vrnext(inst),
            VU0_S2_VRGET => vrget(inst),
            VU0_S2_VRINIT => vrinit(inst),
            VU0_S2_VRXOR => vrxor(inst),
            _ => hooks.emit_unhandled(inst, &format!("Unhandled VU0 Special2 function: 0x{vu_func:X}")),
        };
    }

    match special1_func {
        VU0_S1_VADDx | VU0_S1_VADDy | VU0_S1_VADDz | VU0_S1_VADDw => field_op(inst, "PS2_VADD"),
        VU0_S1_VSUBx | VU0_S1_VSUBy | VU0_S1_VSUBz | VU0_S1_VSUBw => field_op(inst, "PS2_VSUB"),
        VU0_S1_VMULx | VU0_S1_VMULy | VU0_S1_VMULz | VU0_S1_VMULw => field_op(inst, "PS2_VMUL"),
        VU0_S1_VADD => vector_op(inst, "PS2_VADD"),
        VU0_S1_VSUB => vector_op(inst, "PS2_VSUB"),
        VU0_S1_VMUL => vector_op(inst, "PS2_VMUL"),
        VU0_S1_VIADD => integer_op(inst, true, '+'),
        VU0_S1_VISUB => integer_op(inst, true, '-'),
        VU0_S1_VIADDI => viaddi(inst),
        VU0_S1_VIAND => integer_op(inst, false, '&'),
        VU0_S1_VIOR => integer_op(inst, false, '|'),
        VU0_S1_VCALLMS => vcallms(inst),
        VU0_S1_VCALLMSR => vcallmsr(inst, hooks.codegen().vu0_macro_fixes),
        VU0_S1_VADDq => scalar_op(inst, "PS2_VADD", "q"),
        VU0_S1_VSUBq => scalar_op(inst, "PS2_VSUB", "q"),
        VU0_S1_VMULq => scalar_op(inst, "PS2_VMUL", "q"),
        VU0_S1_VADDi => scalar_op(inst, "PS2_VADD", "i"),
        VU0_S1_VSUBi => scalar_op(inst, "PS2_VSUB", "i"),
        VU0_S1_VMULi => scalar_op(inst, "PS2_VMUL", "i"),
        VU0_S1_VMADDx | VU0_S1_VMADDy | VU0_S1_VMADDz | VU0_S1_VMADDw => multiply_accumulate_field(inst, "PS2_VADD"),
        VU0_S1_VMSUBx | VU0_S1_VMSUBy | VU0_S1_VMSUBz | VU0_S1_VMSUBw => multiply_accumulate_field(inst, "PS2_VSUB"),
        VU0_S1_VMAXx | VU0_S1_VMAXy | VU0_S1_VMAXz | VU0_S1_VMAXw => min_max_field(inst, "_mm_max_ps"),
        VU0_S1_VMINIx | VU0_S1_VMINIy | VU0_S1_VMINIz | VU0_S1_VMINIw => min_max_field(inst, "_mm_min_ps"),
        VU0_S1_VMAXi => scalar_op(inst, "_mm_max_ps", "i"),
        VU0_S1_VMINIi => scalar_op(inst, "_mm_min_ps", "i"),
        VU0_S1_VMADD => multiply_accumulate_vector(inst, "PS2_VADD"),
        VU0_S1_VMADDq => multiply_accumulate_scalar(inst, "PS2_VADD", "q"),
        VU0_S1_VMADDi => multiply_accumulate_scalar(inst, "PS2_VADD", "i"),
        VU0_S1_VMAX => plain_vector_op(inst, "_mm_max_ps"),
        VU0_S1_VOPMSUB => vopmsub(inst),
        VU0_S1_VMINI => plain_vector_op(inst, "_mm_min_ps"),
        VU0_S1_VMSUB => multiply_accumulate_vector(inst, "PS2_VSUB"),
        VU0_S1_VMSUBq => multiply_accumulate_scalar(inst, "PS2_VSUB", "q"),
        VU0_S1_VMSUBi => multiply_accumulate_scalar(inst, "PS2_VSUB", "i"),
        _ => hooks.emit_unhandled(inst, &format!("Unhandled VU0 Special1 function: 0x{special1_func:X}")),
    }
}

/// `translateVU_VMAX_Field` / `VMINI_Field`: `res = min/max(vf[vfs], broadcast(vf[vft]))`.
fn min_max_field(inst: &Instruction, op: &str) -> String {
    let (vfd, vfs, vft) = (inst.sa, inst.rd, inst.rt);
    let shuffle = broadcast_shuffle(inst);
    let mask = mask_args(dest_mask(inst));
    format!(
        concat!(
            "{{ __m128 res = {op}(ctx->vu0_vf[{vfs}], _mm_shuffle_ps(ctx->vu0_vf[{vft}], ctx->vu0_vf[{vft}], {shuffle})); ",
            "__m128i mask = _mm_set_epi32({mask}); ",
            "ctx->vu0_vf[{vfd}] = _mm_blendv_ps(ctx->vu0_vf[{vfd}], res, _mm_castsi128_ps(mask)); }}"
        ),
        op = op,
        vfs = vfs,
        vft = vft,
        shuffle = shuffle,
        mask = mask,
        vfd = vfd
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vcallmsr_starts_at_cmsar0_only_with_the_fix() {
        let inst = Instruction { rd: 27, ..Instruction::default() };
        assert!(vcallmsr(&inst, true).contains("ctx->vu0_cmsar0 & 0x1FF"));
        assert!(vcallmsr(&inst, false).contains("ctx->vi[27] & 0x1FF"), "PS2Recomp's text without the switch");
    }

    #[test]
    fn vclipw_puts_the_plus_side_in_bit_0_with_the_fix() {
        let inst = Instruction { function: 0x1FF, ..Instruction::default() };
        let fixed = vclipw(&inst, true);
        assert!(fixed.contains("_mm_set1_epi32(0x7FFFFFFF)"), "compares with |w|: {fixed}");
        assert!(fixed.contains("((gt_mask & 0x1) << 0) | ((lt_mask & 0x1) << 1)"), "{fixed}");
        let compat = vclipw(&inst, false);
        assert!(!compat.contains("0x7FFFFFFF") && compat.contains("((lt_mask & 0x1) << 0)"), "{compat}");
    }
}
