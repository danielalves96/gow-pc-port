//! `vu1n-gen`: the native VU1 block generator.
//!
//! Port of `tools/vu1/vu1n_gen.py`. Reads the 16 KB VU1 micro-memories (built from the ELF by `vu1-manifest apply`, or
//! captured with `GOW_VU1_CAPTURE`), splits them into basic blocks (the same rule as `nativeExtent()` in
//! `ps2_vu1_native.inl`) and writes straight-line C++ for each distinct block, position independent: keyed by its words
//! only, every pc is `n.pc + offset`. Immediate semantics, no cycle model, MAC flags only where they can be observed.
//! Output: `vu1n_part<k>.cpp`. The exact engine (`ps2_vu1_core.cpp`) stays the fallback for every block that is not
//! here and the oracle for `GOW_VU1N_VERIFY`. `vu1s_*.bin` images (declared code never seen uploaded) take every non-empty
//! pair as a block head.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const CODE_SIZE: usize = 16384;
const BRANCH_OPS: [u32; 10] = [0x20, 0x21, 0x24, 0x25, 0x28, 0x29, 0x2C, 0x2D, 0x2E, 0x2F];
const REV4: [u32; 16] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];
/// Latency of an FMAC / CLIP / FCSET result before flag readers see it, in pairs.
const FLAT: usize = 4;

type Pair = (u32, u32);

/// The unmasked pc of the pair at block offset `off`, as a C++ expression (callout argument).
fn pc_arg(off: i64) -> String {
    format!("(n.pc + {off}u)")
}

/// The pc at block offset `off` (may be negative or past the end) wrapped to the 16 KB micro memory.
fn pc_wrap(off: i64) -> String {
    format!("((n.pc + {}u) & 0x3FFFu)", off & 0xFFFF_FFFF)
}

/// `(pc + 16) / 8` for the pair at offset `off`: the value BAL / JALR leave in the link register.
fn link_val(off: i64) -> String {
    format!("((n.pc + {}u) / 8u)", off + 16)
}

fn fnv_block(words: &[u32]) -> u64 {
    let mut hash: u64 = 1_469_598_103_934_665_603;
    for &word in words {
        for i in 0..4 {
            hash = (hash ^ u64::from((word >> (i * 8)) & 0xFF)).wrapping_mul(1_099_511_628_211);
        }
    }
    hash
}

fn sx(value: u32, bits: u32) -> i32 {
    let v = value & ((1u32 << bits) - 1);
    if v & (1 << (bits - 1)) != 0 {
        v as i32 - (1i32 << bits)
    } else {
        v as i32
    }
}

fn is_branch(l: u32, u: u32) -> bool {
    (u >> 31) == 0 && BRANCH_OPS.contains(&(l >> 25))
}

fn pair_at(code: &[u8], position: usize) -> Pair {
    let word = |offset: usize| u32::from_le_bytes([code[offset], code[offset + 1], code[offset + 2], code[offset + 3]]);
    (word(position), word(position + 4))
}

/// Every non-empty pair of a micro-memory image as a block head (images of code nobody has seen entered).
fn scan_heads(code: &[u8]) -> BTreeSet<u32> {
    (0..CODE_SIZE).step_by(8).filter(|&p| pair_at(code, p) != (0, 0)).map(|p| p as u32).collect()
}

/// Pairs (lower, upper) of the block starting at `pc`, same rule as `nativeExtent()`.
fn extent(code: &[u8], pc: usize) -> Vec<Pair> {
    let mut out = Vec::new();
    let mut p = pc;
    while p + 8 <= CODE_SIZE && out.len() < 64 {
        let (l, u) = pair_at(code, p);
        if l == 0 && u == 0 {
            break;
        }
        out.push((l, u));
        if (u >> 28) & 1 != 0 || (u >> 27) & 1 != 0 {
            break;
        }
        if is_branch(l, u) || (u >> 30) & 1 != 0 {
            if p + 16 <= CODE_SIZE {
                let (dl, du) = pair_at(code, p + 8);
                if !(dl == 0 && du == 0) {
                    out.push((dl, du));
                }
            }
            break;
        }
        p += 8;
    }
    out
}

fn heads_of(code: &[u8], extra_heads: &BTreeSet<u32>) -> BTreeSet<u32> {
    let mut heads = extra_heads.clone();
    let pairs: Vec<Pair> = (0..CODE_SIZE / 8).map(|i| pair_at(code, i * 8)).collect();
    for (i, &(l, u)) in pairs.iter().enumerate() {
        if l == 0 && u == 0 {
            continue;
        }
        let pc = (i * 8) as i64;
        if i == 0 || pairs[i - 1] == (0, 0) {
            heads.insert(pc as u32);
        }
        if is_branch(l, u) {
            let op = l >> 25;
            if op != 0x24 && op != 0x25 {
                heads.insert(((pc + 8 + i64::from(sx(l, 11)) * 8) & 0x3FFF) as u32);
            }
            heads.insert(((pc + 16) & 0x3FFF) as u32);
        }
        if (u >> 30) & 1 != 0 {
            heads.insert(((pc + 16) & 0x3FFF) as u32);
        }
        if (u >> 28) & 1 != 0 || (u >> 27) & 1 != 0 {
            heads.insert(((pc + 8) & 0x3FFF) as u32);
        }
    }
    heads
}

// ---------------------------------------------------------------------------------------------------------------------
// upper (FMAC) decode

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Add,
    Sub,
    Madd,
    Msub,
    Max,
    Mini,
    Mul,
    Itof,
    Ftoi,
    Abs,
    Clip,
    Nop,
    Opmsub,
    Opmula,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Source {
    Bc,
    Q,
    I,
    V,
    None,
    Shift(u32),
}

#[derive(Clone, Copy, Debug)]
struct Class {
    kind: Kind,
    source: Source,
    to_acc: bool,
    bc: u32,
}

const fn class(kind: Kind, source: Source, to_acc: bool, bc: u32) -> Class {
    Class { kind, source, to_acc, bc }
}

/// `(kind, source, toAcc, bc)` like `vu1c::classify`, plus special kinds.
fn classify(u: u32) -> Class {
    use Kind::*;
    use Source::{Bc, I, Q, V};
    let op = u & 0x3F;
    if op < 0x3C {
        let c = op;
        let bc = c & 3;
        return match c {
            0x00..=0x03 => class(Add, Bc, false, bc),
            0x04..=0x07 => class(Sub, Bc, false, bc),
            0x08..=0x0B => class(Madd, Bc, false, bc),
            0x0C..=0x0F => class(Msub, Bc, false, bc),
            0x10..=0x13 => class(Max, Bc, false, bc),
            0x14..=0x17 => class(Mini, Bc, false, bc),
            0x18..=0x1B => class(Mul, Bc, false, bc),
            0x1C => class(Mul, Q, false, 0),
            0x1D => class(Max, I, false, 0),
            0x1E => class(Mul, I, false, 0),
            0x1F => class(Mini, I, false, 0),
            0x20 => class(Add, Q, false, 0),
            0x21 => class(Madd, Q, false, 0),
            0x22 => class(Add, I, false, 0),
            0x23 => class(Madd, I, false, 0),
            0x24 => class(Sub, Q, false, 0),
            0x25 => class(Msub, Q, false, 0),
            0x26 => class(Sub, I, false, 0),
            0x27 => class(Msub, I, false, 0),
            0x28 => class(Add, V, false, 0),
            0x29 => class(Madd, V, false, 0),
            0x2A => class(Mul, V, false, 0),
            0x2B => class(Max, V, false, 0),
            0x2C => class(Sub, V, false, 0),
            0x2D => class(Msub, V, false, 0),
            0x2E => class(Opmsub, V, false, 0),
            0x2F => class(Mini, V, false, 0),
            _ => class(Other, V, false, 0),
        };
    }
    let sp = (u & 3) | ((u >> 4) & 0x7C);
    let shifts = [0u32, 4, 12, 15];
    match sp {
        0x00..=0x03 => class(Add, Bc, true, sp & 3),
        0x04..=0x07 => class(Sub, Bc, true, sp & 3),
        0x08..=0x0B => class(Madd, Bc, true, sp & 3),
        0x0C..=0x0F => class(Msub, Bc, true, sp & 3),
        0x10..=0x13 => class(Itof, Source::Shift(shifts[(sp & 3) as usize]), false, 0),
        0x14..=0x17 => class(Ftoi, Source::Shift(shifts[(sp & 3) as usize]), false, 0),
        0x18..=0x1B => class(Mul, Bc, true, sp & 3),
        0x1D => class(Abs, Source::None, false, 0),
        0x1F => class(Clip, Source::None, false, 0),
        0x2F | 0x30 => class(Nop, Source::None, false, 0),
        0x1C => class(Mul, Q, true, 0),
        0x1E => class(Mul, I, true, 0),
        0x20 => class(Add, Q, true, 0),
        0x21 => class(Madd, Q, true, 0),
        0x22 => class(Add, I, true, 0),
        0x23 => class(Madd, I, true, 0),
        0x24 => class(Sub, Q, true, 0),
        0x25 => class(Msub, Q, true, 0),
        0x26 => class(Sub, I, true, 0),
        0x27 => class(Msub, I, true, 0),
        0x28 => class(Add, V, true, 0),
        0x29 => class(Madd, V, true, 0),
        0x2A => class(Mul, V, true, 0),
        0x2C => class(Sub, V, true, 0),
        0x2D => class(Msub, V, true, 0),
        0x2E => class(Opmula, V, true, 0),
        _ => class(Other, V, false, 0),
    }
}

struct UpperInfo {
    writes: BTreeSet<u32>,
    reads: BTreeSet<u32>,
    writes_mac: bool,
    reads_q: bool,
}

fn upper_info(u: u32) -> UpperInfo {
    let Class { kind, source, to_acc, .. } = classify(u);
    let dest = (u >> 21) & 0xF;
    let (ft, fs, fd) = ((u >> 16) & 31, (u >> 11) & 31, (u >> 6) & 31);
    let mut w: BTreeSet<u32> = BTreeSet::new();
    let mut r: BTreeSet<u32> = BTreeSet::new();
    if kind == Kind::Nop || (dest == 0 && kind != Kind::Clip) {
        return UpperInfo { writes: w, reads: r, writes_mac: false, reads_q: false };
    }
    match kind {
        Kind::Add | Kind::Sub | Kind::Madd | Kind::Msub | Kind::Mul | Kind::Max | Kind::Mini => {
            r.insert(fs);
            if matches!(source, Source::Bc | Source::V) {
                r.insert(ft);
            }
            if !to_acc {
                w.insert(fd);
            }
        }
        Kind::Itof | Kind::Ftoi | Kind::Abs => {
            r.insert(fs);
            w.insert(ft);
        }
        Kind::Clip => {
            r.insert(fs);
            r.insert(ft);
        }
        Kind::Opmsub | Kind::Opmula | Kind::Other => {
            r.insert(fs);
            r.insert(ft);
            if !to_acc {
                w.insert(fd);
            }
        }
        Kind::Nop => {}
    }
    let writes_mac = matches!(kind, Kind::Add | Kind::Sub | Kind::Madd | Kind::Msub | Kind::Mul) && dest != 0;
    let reads_q = source == Source::Q;
    UpperInfo { writes: w.into_iter().filter(|&x| x != 0).collect(), reads: r, writes_mac, reads_q }
}

/// GOW-Port: what the upper emitter knows about the block's VF operands: the registers holding clean (normalised) results, and
/// the locals that already hold a normalised register (planning 3.2).
struct Operands<'a> {
    clean: &'a BTreeSet<u32>,
    locals: &'a mut BTreeMap<u32, String>,
}

fn emit_upper(
    u: u32,
    pc: &str,
    idx: usize,
    mac_var: Option<&str>,
    clip_var: &str,
    prev_clip: &str,
    operands: Operands<'_>,
) -> Vec<String> {
    let Operands { clean, locals: operands } = operands;
    let Class { kind, source, to_acc, bc } = classify(u);
    let dest = (u >> 21) & 0xF;
    let (ft, fs, fd) = ((u >> 16) & 31, (u >> 11) & 31, (u >> 6) & 31);
    let m = REV4[dest as usize];
    if kind == Kind::Nop {
        return Vec::new();
    }
    if kind == Kind::Clip {
        return vec![format!("const uint32_t {clip_var} = vu1n::clipVal(s, {fs}, {ft}, {prev_clip});")];
    }
    if dest == 0 {
        return Vec::new();
    }
    let shift = if let Source::Shift(s) = source { s } else { 0 };
    match kind {
        Kind::Itof => {
            return if ft != 0 { vec![format!("vu1n::itof<{shift}, {m}>(s, {fs}, {ft});")] } else { Vec::new() }
        }
        Kind::Ftoi => {
            return if ft != 0 { vec![format!("vu1n::ftoi<{shift}, {m}>(s, {fs}, {ft});")] } else { Vec::new() }
        }
        Kind::Abs => return if ft != 0 { vec![format!("vu1n::absOp<{m}>(s, {fs}, {ft});")] } else { Vec::new() },
        Kind::Opmsub | Kind::Opmula | Kind::Other => {
            return vec![format!("VU1NativeAccess::callUpper(n, {pc}, 0x{u:08x}u);")]
        }
        _ => {}
    }
    // GOW-Port (planning 3.2): a register normalised once per block: the first read declares a local, later reads use it
    // until the register is written (the caller drops it then, see `operands`).
    let mut loads: Vec<String> = Vec::new();
    let mut opnd = |r: u32| {
        if r == 0 || clean.contains(&r) {
            format!("vu1n::ld(s, {r})")
        } else if let Some(local) = operands.get(&r) {
            local.clone()
        } else {
            let local = format!("v{r}_{idx}");
            loads.push(format!("const __m128 {local} = vu1n::ldn(s, {r});"));
            operands.insert(r, local.clone());
            local
        }
    };
    let a = opnd(fs);
    let b = match source {
        Source::Bc => format!("vu1n::bc<{bc}>({})", opnd(ft)),
        Source::Q => "_mm_set1_ps(s.q)".to_owned(),
        Source::I => "_mm_set1_ps(s.i)".to_owned(),
        _ => opnd(ft),
    };
    let acc = "vu1n::ldAcc(s)";
    let r = match kind {
        Kind::Add => format!("_mm_add_ps({a}, {b})"),
        Kind::Sub => format!("_mm_sub_ps({a}, {b})"),
        Kind::Mul => format!("_mm_mul_ps({a}, {b})"),
        Kind::Madd => format!("vu1n::madd<false>({acc}, {a}, {b})"),
        Kind::Msub => format!("vu1n::madd<true>({acc}, {a}, {b})"),
        Kind::Max => format!("_mm_max_ps({a}, {b})"),
        _ => format!("_mm_min_ps({a}, {b})"),
    };
    let target = if to_acc { "s.acc".to_owned() } else { format!("s.vf[{fd}]") };
    let mut out = loads;
    out.push(format!("__m128 r{idx} = {r};"));
    if matches!(kind, Kind::Add | Kind::Sub | Kind::Mul | Kind::Madd | Kind::Msub) {
        if let Some(mac_var) = mac_var {
            out.push(format!("const uint32_t {mac_var} = vu1n::macVal<{m}>(r{idx});"));
            out.push(format!(
                "const __m128 q{idx} = r{idx};  // unclamped: flags computed later if it outlives the block"
            ));
        }
    }
    // GOW-Port (planning 3.2): no clamp after ADD / SUB / MUL. The blocks run with round toward zero (runNative sets MXCSR),
    // where an overflow gives +-FLT_MAX, never Inf, and the operands are finite (normalised, clean results, vf00, Q, I), so
    // NaN cannot appear either: the clamp left every value as it was. MADD / MSUB keep it (their wide path is in double).
    if matches!(kind, Kind::Madd | Kind::Msub) {
        out.push(format!("r{idx} = vu1n::clampv(r{idx});"));
    }
    if to_acc || fd != 0 {
        out.push(format!("vu1n::stf<{m}>({target}, r{idx});"));
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// lower decode

struct LowerFields {
    dest: u32,
    ft: u32,
    fs: u32,
    vid: u32,
    imm11: i32,
    imm15: u32,
    imm24: u32,
}

fn lower_fields(l: u32) -> LowerFields {
    LowerFields {
        dest: (l >> 21) & 0xF,
        ft: (l >> 16) & 31,
        fs: (l >> 11) & 31,
        vid: (l >> 6) & 31,
        imm11: sx(l, 11),
        imm15: (l & 0x7FF) | ((l >> 10) & 0x7800),
        imm24: l & 0xFF_FFFF,
    }
}

fn lower_special(l: u32) -> Option<u32> {
    ((l & 0x3F) >= 0x3C).then_some((l & 3) | ((l >> 4) & 0x7C))
}

#[derive(Default)]
struct LowerInfo {
    vfw: BTreeSet<u32>,
    vfr: BTreeSet<u32>,
    writes_q: bool,
    /// The register whose write delays a following branch read.
    delay: u32,
    vi_reads: BTreeSet<u32>,
}

fn set_of(items: &[u32]) -> BTreeSet<u32> {
    items.iter().copied().collect()
}

fn lower_info(l: u32) -> LowerInfo {
    let op = l >> 25;
    let f = lower_fields(l);
    let mut info = LowerInfo::default();
    if l == 0 || l == 0x8000_033C {
        return info;
    }
    let (ft, fs, vid) = (f.ft, f.fs, f.vid);
    match op {
        0x00 => {
            info.vfw = set_of(&[ft]);
            info.vi_reads = set_of(&[fs]);
        }
        0x01 => {
            info.vfr = set_of(&[fs]);
            info.vi_reads = set_of(&[ft]);
        }
        0x04 | 0x05 => info.vi_reads = if op == 0x05 { set_of(&[fs, ft]) } else { set_of(&[fs]) },
        0x08 | 0x09 => {
            info.delay = ft;
            info.vi_reads = set_of(&[fs]);
        }
        0x10..=0x13 | 0x1C => {}
        0x18 | 0x1A | 0x1B => info.vi_reads = set_of(&[fs]),
        _ if BRANCH_OPS.contains(&op) => {
            info.vi_reads = if matches!(op, 0x28 | 0x29) {
                set_of(&[fs, ft])
            } else if matches!(op, 0x24 | 0x25 | 0x2C | 0x2D | 0x2E | 0x2F) {
                set_of(&[fs])
            } else {
                BTreeSet::new()
            };
        }
        0x40 => {
            let function = l & 0x3F;
            if matches!(function, 0x30 | 0x31 | 0x34 | 0x35) {
                info.delay = vid;
                info.vi_reads = set_of(&[fs, ft]);
            } else if function == 0x32 {
                info.delay = ft;
                info.vi_reads = set_of(&[fs]);
            } else if function >= 0x3C {
                match lower_special(l) {
                    Some(0x30 | 0x31) => {
                        info.vfw = set_of(&[ft]);
                        info.vfr = set_of(&[fs]);
                    }
                    Some(0x34 | 0x36) => {
                        info.vfw = set_of(&[ft]);
                        info.delay = fs;
                        info.vi_reads = set_of(&[fs]);
                    }
                    Some(0x35 | 0x37) => {
                        info.vfr = set_of(&[fs]);
                        info.delay = ft;
                        info.vi_reads = set_of(&[ft]);
                    }
                    Some(0x38 | 0x3A) => {
                        info.vfr = set_of(&[fs, ft]);
                        info.writes_q = true;
                    }
                    Some(0x39) => {
                        info.vfr = set_of(&[ft]);
                        info.writes_q = true;
                    }
                    Some(0x3B) => {}
                    Some(0x3C) => {
                        info.vfr = set_of(&[fs]);
                        info.delay = ft;
                    }
                    Some(0x3D) => {
                        info.vfw = set_of(&[ft]);
                        info.vi_reads = set_of(&[fs]);
                    }
                    Some(0x3E | 0x3F) => info.vi_reads = set_of(&[fs, ft]),
                    Some(0x64) => info.vfw = set_of(&[ft]),
                    Some(0x68 | 0x69) => {}
                    Some(0x6C) => info.vi_reads = set_of(&[fs]),
                    _ => {
                        info.vfr = set_of(&[fs, ft]);
                        info.vfw = set_of(&[ft]);
                    }
                }
            }
        }
        _ => {
            info.vfr = set_of(&[fs, ft]);
            info.vfw = set_of(&[ft]);
        }
    }
    info.vfw.remove(&0);
    info
}

fn dest_comp(dest: u32) -> u32 {
    if dest & 8 != 0 {
        0
    } else if dest & 4 != 0 {
        1
    } else if dest & 2 != 0 {
        2
    } else {
        3
    }
}

fn vi_set(reg: u32, expr: &str) -> Vec<String> {
    if reg != 0 {
        vec![format!("s.vi[{reg}] = {expr};")]
    } else {
        Vec::new()
    }
}

/// Lines for a non-branch lower op.
fn emit_lower(l: u32, pc: &str, upper: u32, mac_expr: &str, clip_expr: &str, clip_var: &str) -> Vec<String> {
    let op = l >> 25;
    let f = lower_fields(l);
    let (dest, ft, fs, vid, imm11) = (f.dest, f.ft, f.fs, f.vid, f.imm11);
    let m = REV4[dest as usize];
    if l == 0 || l == 0x8000_033C {
        return Vec::new();
    }
    match op {
        0x00 => {
            return if ft != 0 {
                vec![format!("{{ uint32_t a = vu1n::qaddr(n, s.vi[{fs}] + {imm11}); vu1n::stf<{m}>(s.vf[{ft}], vu1n::ldq(n, a)); }}")]
            } else {
                Vec::new()
            };
        }
        0x01 => {
            return vec![format!(
                "{{ uint32_t a = vu1n::qaddr(n, s.vi[{ft}] + {imm11}); vu1n::stq<{m}>(n, a, vu1n::ld(s, {fs})); }}"
            )]
        }
        0x04 => {
            return vi_set(ft, &format!("vu1n::ldWord(n, vu1n::qaddr(n, s.vi[{fs}] + {imm11}), {})", dest_comp(dest)))
        }
        0x05 => return vec![format!("vu1n::stWords(n, vu1n::qaddr(n, s.vi[{fs}] + {imm11}), s.vi[{ft}], {dest});")],
        0x08 => return vi_set(ft, &format!("vu1n::sx16(s.vi[{fs}] + {})", f.imm15)),
        0x09 => return vi_set(ft, &format!("vu1n::sx16(s.vi[{fs}] - {})", f.imm15)),
        0x10 => return vi_set(1, &format!("(({clip_expr} & 0xFFFFFFu) == 0x{:x}u) ? 1 : 0", f.imm24)),
        0x11 => return vec![format!("const uint32_t {clip_var} = 0x{:x}u;", f.imm24)],
        0x12 => return vi_set(1, &format!("(({clip_expr} & 0x{:x}u) != 0u) ? 1 : 0", f.imm24)),
        0x13 => return vi_set(1, &format!("(({clip_expr} | 0x{:x}u) == 0xFFFFFFu) ? 1 : 0", f.imm24)),
        0x18 => return vi_set(ft, &format!("(({mac_expr} & 0xFFFFu) == (uint32_t)(uint16_t)s.vi[{fs}]) ? 1 : 0")),
        0x1A => return vi_set(ft, &format!("(int32_t)({mac_expr} & (uint32_t)(uint16_t)s.vi[{fs}])")),
        0x1B => return vi_set(ft, &format!("(int32_t)({mac_expr} | (uint32_t)(uint16_t)s.vi[{fs}])")),
        0x1C => return vi_set(ft, &format!("(int32_t)({clip_expr} & 0x0FFFu)")),
        0x40 => {
            let function = l & 0x3F;
            match function {
                0x30 => return vi_set(vid, &format!("vu1n::sx16(s.vi[{fs}] + s.vi[{ft}])")),
                0x31 => return vi_set(vid, &format!("vu1n::sx16(s.vi[{fs}] - s.vi[{ft}])")),
                0x32 => return vi_set(ft, &format!("vu1n::sx16(s.vi[{fs}] + {})", sx((l >> 6) & 0x1F, 5))),
                0x34 => return vi_set(vid, &format!("s.vi[{fs}] & s.vi[{ft}]")),
                0x35 => return vi_set(vid, &format!("s.vi[{fs}] | s.vi[{ft}]")),
                _ => {}
            }
            let sp = lower_special(l);
            let (fsf, ftf) = ((l >> 21) & 3, (l >> 23) & 3);
            match sp {
                Some(0x30) => {
                    return if ft != 0 {
                        vec![format!("vu1n::stf<{m}>(s.vf[{ft}], vu1n::ld(s, {fs}));")]
                    } else {
                        Vec::new()
                    };
                }
                Some(0x31) => {
                    return if ft != 0 {
                        vec![format!(
                            "{{ __m128 v = vu1n::ld(s, {fs}); vu1n::stf<{m}>(s.vf[{ft}], _mm_shuffle_ps(v, v, _MM_SHUFFLE(0, 3, 2, 1))); }}"
                        )]
                    } else {
                        Vec::new()
                    };
                }
                Some(0x34) => {
                    // LQI
                    let mut out = Vec::new();
                    if ft != 0 {
                        out.push(format!("{{ uint32_t a = vu1n::qaddr(n, (uint16_t)s.vi[{fs}]); vu1n::stf<{m}>(s.vf[{ft}], vu1n::ldq(n, a)); }}"));
                    }
                    if fs != 0 {
                        out.push(format!("s.vi[{fs}] = vu1n::sx16(s.vi[{fs}] + 1);"));
                    }
                    return out;
                }
                Some(0x35) => {
                    // SQI
                    let mut out = vec![format!("{{ uint32_t a = vu1n::qaddr(n, (uint16_t)s.vi[{ft}]); vu1n::stq<{m}>(n, a, vu1n::ld(s, {fs})); }}")];
                    if ft != 0 {
                        out.push(format!("s.vi[{ft}] = vu1n::sx16(s.vi[{ft}] + 1);"));
                    }
                    return out;
                }
                Some(0x36) => {
                    // LQD
                    let mut out = Vec::new();
                    if fs != 0 {
                        out.push(format!("s.vi[{fs}] = vu1n::sx16(s.vi[{fs}] - 1);"));
                    }
                    if ft != 0 {
                        out.push(format!("{{ uint32_t a = vu1n::qaddr(n, (uint16_t)s.vi[{fs}]); vu1n::stf<{m}>(s.vf[{ft}], vu1n::ldq(n, a)); }}"));
                    }
                    return out;
                }
                Some(0x37) => {
                    // SQD
                    let mut out = Vec::new();
                    if ft != 0 {
                        out.push(format!("s.vi[{ft}] = vu1n::sx16(s.vi[{ft}] - 1);"));
                    }
                    out.push(format!("{{ uint32_t a = vu1n::qaddr(n, (uint16_t)s.vi[{ft}]); vu1n::stq<{m}>(n, a, vu1n::ld(s, {fs})); }}"));
                    return out;
                }
                Some(0x38) => return vec![format!("vu1n::divOp(s, {fs}, {fsf}, {ft}, {ftf});")],
                Some(0x39) => return vec![format!("vu1n::sqrtOp(s, {ft}, {ftf});")],
                Some(0x3A) => return vec![format!("vu1n::rsqrtOp(s, {fs}, {fsf}, {ft}, {ftf});")],
                Some(0x3B) => return Vec::new(),
                Some(0x3C) => return vi_set(ft, &format!("vu1n::compBits(s, {fs}, {fsf})")),
                Some(0x3D) => {
                    return if ft != 0 {
                        vec![format!("vu1n::stf<{m}>(s.vf[{ft}], vu1n::fromInt(vu1n::sx16(s.vi[{fs}])));")]
                    } else {
                        Vec::new()
                    };
                }
                Some(0x3E) => {
                    return vi_set(
                        ft,
                        &format!("vu1n::ldWord(n, vu1n::qaddr(n, (uint16_t)s.vi[{fs}]), {})", dest_comp(dest)),
                    );
                }
                Some(0x3F) => {
                    return vec![format!("vu1n::stWords(n, vu1n::qaddr(n, (uint16_t)s.vi[{fs}]), s.vi[{ft}], {dest});")]
                }
                Some(0x64) => {
                    return if ft != 0 {
                        vec![format!("vu1n::stf<{m}>(s.vf[{ft}], _mm_set1_ps(s.p));")]
                    } else {
                        Vec::new()
                    };
                }
                Some(0x68) => return vi_set(ft, "(int32_t)(s.top & 0x3FFu)"),
                Some(0x69) => return vi_set(ft, "(int32_t)(s.itop & 0x3FFu)"),
                Some(0x6C) => return vec![format!("VU1NativeAccess::xgkick(n, (uint32_t)(uint16_t)s.vi[{fs}]);")],
                _ => {}
            }
        }
        _ => {}
    }
    vec![format!("VU1NativeAccess::callLower(n, {pc}, 0x{l:08x}u, 0x{upper:08x}u);")]
}

/// A block's tail record: the MAC pairs, their masks, and the clip pairs that outlive it.
type Tail = (Vec<usize>, Vec<u32>, Vec<usize>);

/// C++ body for a block, or `None` if the block uses something the native backend does not support.
fn gen_block(pairs: &[Pair], tail_out: &mut Vec<Tail>) -> Option<Vec<String>> {
    let n = pairs.len();
    let infos: Vec<LowerInfo> =
        pairs.iter().map(|&(l, u)| if u >> 31 == 0 { lower_info(l) } else { lower_info(0) }).collect();
    let uinfo: Vec<UpperInfo> = pairs.iter().map(|&(_, u)| upper_info(u)).collect();

    // Structural checks.
    for (i, &(l, u)) in pairs.iter().enumerate() {
        if is_branch(l, u) {
            if i != n.wrapping_sub(2) {
                return None; // the branch must be the second to last pair (delay slot last)
            }
            if (u >> 30) & 1 != 0 || (u >> 28) & 1 != 0 || (u >> 27) & 1 != 0 {
                return None;
            }
            if is_branch(pairs[n - 1].0, pairs[n - 1].1) {
                return None; // branch in a delay slot
            }
        }
    }
    let last_is_branch_block = n >= 2 && is_branch(pairs[n - 2].0, pairs[n - 2].1);
    if n >= 1 && is_branch(pairs[n - 1].0, pairs[n - 1].1) && !last_is_branch_block {
        return None; // branch without a delay slot
    }

    let mac_w: Vec<usize> = (0..n).filter(|&i| uinfo[i].writes_mac).collect();
    let clip_w: Vec<usize> = (0..n)
        .filter(|&i| classify(pairs[i].1).kind == Kind::Clip || ((pairs[i].1 >> 31) == 0 && (pairs[i].0 >> 25) == 0x11))
        .collect();
    for &i in &clip_w {
        if classify(pairs[i].1).kind == Kind::Clip && (pairs[i].1 >> 31) == 0 && (pairs[i].0 >> 25) == 0x11 {
            return None;
        }
    }

    // The newest earlier writer already committed when pair `i` issues: pair distance >= 4 always, nearer ones only
    // when the exact engine's stall model (n.cyc) pushed pair `i` far enough.
    let visible = |i: usize, writers: &[usize], prefix: &str, entry: String| -> String {
        let mut expr = entry;
        for &j in writers.iter().filter(|&&j| j < i) {
            if i - j >= FLAT {
                expr = format!("{prefix}{j}");
            } else {
                expr = format!("((n.cyc[{i}] - n.cyc[{j}] >= {FLAT}) ? {prefix}{j} : {expr})");
            }
        }
        expr
    };
    let mac_src = |i: usize| visible(i, &mac_w, "m", format!("VU1NativeAccess::macAt(n, n.cyc[{i}])"));
    let clip_src = |i: usize| visible(i, &clip_w, "c", format!("VU1NativeAccess::clipAt(n, n.cyc[{i}])"));
    let clip_prev = |i: usize| -> String {
        match clip_w.iter().rev().find(|&&j| j < i) {
            Some(j) => format!("c{j}"),
            None => "VU1NativeAccess::clipChain(n)".to_owned(),
        }
    };
    let mask_of = |j: usize| REV4[((pairs[j].1 >> 21) & 0xF) as usize];

    // Block end: the newest MAC / clip writer is queued with the block's tail record (n.tail); the older writers that can
    // still be the visible one after the exit are only stored there, the reader decides (VU1FlagRing::tailVisible).
    let flush_final = |i: usize, tail_out: &mut Vec<Tail>| -> Vec<String> {
        let mut out = Vec::new();
        let mut desc_m: Vec<usize> = Vec::new();
        let mut desc_c: Vec<usize> = Vec::new();
        for (writers, prefix) in [(&mac_w, "m"), (&clip_w, "c")] {
            let w: Vec<usize> = writers.iter().copied().filter(|&j| j <= i).collect();
            let older: Vec<usize> =
                (0..w.len().saturating_sub(1)).filter(|&k| i - w[k + 1] < FLAT - 1).map(|k| w[k]).collect();
            for (k, j) in older.iter().enumerate() {
                out.push(if prefix == "m" {
                    format!("n.tail->q[{k}] = q{j};")
                } else {
                    format!("n.tail->c[{k}] = c{j};")
                });
            }
            if prefix == "m" {
                desc_m = older;
            } else {
                desc_c = older;
            }
        }
        let has_tail = !desc_m.is_empty() || !desc_c.is_empty();
        let tail = if has_tail { "n.tail" } else { "nullptr" };
        if let Some(&j) = mac_w.last().filter(|&&j| j <= i) {
            out.push(format!("VU1NativeAccess::pushMacRaw(n, n.cyc[{j}], q{j}, {}u, {tail});", mask_of(j)));
        }
        if let Some(&j) = clip_w.last().filter(|&&j| j <= i) {
            out.push(format!("VU1NativeAccess::pushClipTail(n, n.cyc[{j}], c{j}, {tail});"));
        }
        if has_tail {
            let masks = desc_m.iter().map(|&j| mask_of(j)).collect();
            tail_out.push((desc_m, masks, desc_c));
        }
        out
    };

    // Block exit after pair `i` (D/T halt): queue every writer that can still be the visible one at or after the exit.
    let flush = |i: usize| -> Vec<String> {
        let mut out = Vec::new();
        let end = format!("n.cyc[{i}] + 1u");
        for (writers, prefix, push) in [(&mac_w, "m", "pushMac"), (&clip_w, "c", "pushClip")] {
            let w: Vec<usize> = writers.iter().copied().filter(|&j| j <= i).collect();
            for (k, &j) in w.iter().enumerate() {
                let call = if prefix == "m" {
                    // MAC flags of a writer read after the block: computed by the reader (VU1FlagRing).
                    format!("VU1NativeAccess::pushMacRaw(n, n.cyc[{j}], q{j}, {}u, nullptr);", mask_of(j))
                } else {
                    format!("VU1NativeAccess::{push}(n, n.cyc[{j}], {prefix}{j});")
                };
                if k == w.len() - 1 {
                    out.push(call);
                } else if i - w[k + 1] < FLAT - 1 {
                    // Visible after the exit only if its successor is not yet: issue-cycle gaps are at least the pair
                    // distance (stalls only widen them), so writers followed by one 3+ pairs before the end never are.
                    out.push(format!("if (n.cyc[{}] + {FLAT}u > {end}) {call}", w[k + 1]));
                }
            }
        }
        out
    };

    let mut lines: Vec<String> = Vec::new();
    let mut clean_regs: BTreeSet<u32> = BTreeSet::new(); // vf registers known to hold normalised values: FMAC results of this block
    let mut operands: BTreeMap<u32, String> = BTreeMap::new(); // normalised operand locals still valid (GOW-Port, planning 3.2)
    for (i, &(l, u)) in pairs.iter().enumerate() {
        let off = (8 * i) as i64;
        let clean_i: BTreeSet<u32> = clean_regs.difference(&infos[i].vfw).copied().collect();
        let li = &infos[i];
        let ui = &uinfo[i];
        let i_bit = (u >> 31) != 0;
        let mut lower_first = false;
        if !i_bit {
            let h1 = ui.writes.intersection(&li.vfr).next().is_some(); // upper writes what lower reads (lower must read old)
            let h2 = li.vfw.intersection(&ui.reads).next().is_some(); // lower writes what upper reads (upper must read old)
            let h3 = li.writes_q && ui.reads_q; // lower writes Q, upper reads old Q
            if h1 && (h2 || h3) {
                return None;
            }
            lower_first = h1;
        }
        // Backup of a VI register for a branch in the next pair.
        let mut backup = Vec::new();
        if i + 1 < n && li.delay != 0 {
            let (nl, nu) = pairs[i + 1];
            if is_branch(nl, nu) && lower_info(nl).vi_reads.contains(&li.delay) {
                backup.push(format!("const int32_t vib{} = s.vi[{}];", i + 1, li.delay));
            }
        }
        lines.push(format!("// pair {i} +0x{:04x} lower={l:08x} upper={u:08x}", 8 * i));
        lines.extend(backup);
        let mac_var = if mac_w.contains(&i) { Some(format!("m{i}")) } else { None };
        let clip_var = format!("c{i}");
        if is_branch(l, u) {
            let op = l >> 25;
            let f = lower_fields(l);
            let (ft, fs, imm) = (f.ft, f.fs, i64::from(f.imm11));
            let rd = |reg: u32| -> String {
                if i > 0 && infos[i - 1].delay == reg && reg != 0 {
                    format!("vib{i}")
                } else {
                    format!("s.vi[{reg}]")
                }
            };
            let target = pc_wrap(off + 8 + imm * 8);
            match op {
                0x20 => {
                    lines.push("bool taken = true;".to_owned());
                    lines.push(format!("uint32_t target = {target};"));
                }
                0x21 => {
                    if ft != 0 {
                        lines.push(format!("s.vi[{ft}] = {};", link_val(off)));
                    }
                    lines.push("bool taken = true;".to_owned());
                    lines.push(format!("uint32_t target = {target};"));
                }
                0x24 => {
                    lines.push("bool taken = true;".to_owned());
                    lines.push(format!("uint32_t target = (((uint32_t)(uint16_t)s.vi[{fs}]) * 8u) & 0x3FFFu;"));
                }
                0x25 => {
                    lines.push(format!("uint32_t target = (((uint32_t)(uint16_t)s.vi[{fs}]) * 8u) & 0x3FFFu;"));
                    if ft != 0 {
                        lines.push(format!("s.vi[{ft}] = {};", link_val(off)));
                    }
                    lines.push("bool taken = true;".to_owned());
                }
                _ => {
                    let cond = match op {
                        0x28 => format!("(int16_t){} == (int16_t){}", rd(fs), rd(ft)),
                        0x29 => format!("(int16_t){} != (int16_t){}", rd(fs), rd(ft)),
                        0x2C => format!("(int16_t){} < 0", rd(fs)),
                        0x2D => format!("(int16_t){} > 0", rd(fs)),
                        0x2E => format!("(int16_t){} <= 0", rd(fs)),
                        _ => format!("(int16_t){} >= 0", rd(fs)),
                    };
                    lines.push(format!("bool taken = {cond};"));
                    lines.push(format!("uint32_t target = {target};"));
                }
            }
            // The upper still executes in the branch pair.
            let up = emit_upper(
                u,
                &pc_arg(off),
                i,
                mac_var.as_deref(),
                &clip_var,
                &clip_prev(i),
                Operands { clean: &clean_i, locals: &mut operands },
            );
            // The delay slot that follows must not read a local of a register this upper wrote.
            if up.iter().any(|line| line.contains("VU1NativeAccess::call")) {
                operands.clear();
            } else {
                for r in uinfo[i].writes.iter() {
                    operands.remove(r);
                }
            }
            lines.extend(up);
            continue;
        }

        // Ordinary pair.
        let up = emit_upper(
            u,
            &pc_arg(off),
            i,
            mac_var.as_deref(),
            &clip_var,
            &clip_prev(i),
            Operands { clean: &clean_i, locals: &mut operands },
        );
        let lo = if i_bit { Vec::new() } else { emit_lower(l, &pc_arg(off), u, &mac_src(i), &clip_src(i), &clip_var) };
        // A register this pair writes no longer holds the normalised local; a call into the interpreter (an upper or lower the
        // generator does not emit) may write any register.
        if up.iter().chain(lo.iter()).any(|line| line.contains("VU1NativeAccess::call")) {
            operands.clear();
        } else {
            for r in infos[i].vfw.iter().chain(uinfo[i].writes.iter()) {
                operands.remove(r);
            }
        }
        if i_bit {
            lines.extend(up);
            lines.push(format!("{{ const uint32_t bits = 0x{l:08x}u; float imm; std::memcpy(&imm, &bits, 4); s.i = vu1n::qnorm(imm); }}"));
        } else if lower_first {
            lines.extend(lo);
            lines.extend(up);
        } else {
            lines.extend(up);
            lines.extend(lo);
        }
        clean_regs = clean_i;
        let class_u = classify(u);
        if matches!(class_u.kind, Kind::Add | Kind::Sub | Kind::Mul | Kind::Madd | Kind::Msub | Kind::Max | Kind::Mini)
            && ((u >> 21) & 0xF) == 0xF
            && !class_u.to_acc
        {
            clean_regs.extend(ui.writes.iter().copied());
        }
        // D/T halt bits.
        if (u >> 28) & 1 != 0 {
            lines.push(format!(
                "if (s.dBitEnabled) {{ {} s.stoppedByD = true; return kVu1nEnded | {}; }}",
                flush(i).join(" "),
                pc_wrap(off + 8)
            ));
        }
        if (u >> 27) & 1 != 0 {
            lines.push(format!(
                "if (s.tBitEnabled) {{ {} s.stoppedByT = true; return kVu1nEnded | {}; }}",
                flush(i).join(" "),
                pc_wrap(off + 8)
            ));
        }
    }
    let end_pc = pc_wrap((8 * n) as i64);
    let e_bit = pairs.iter().any(|&(_, u)| (u >> 30) & 1 != 0);
    lines.extend(flush_final(n - 1, tail_out));
    if last_is_branch_block {
        lines.push(format!("return taken ? target : {end_pc};"));
    } else if e_bit {
        lines.push(format!("return kVu1nEnded | {end_pc};"));
    } else {
        lines.push(format!("return {end_pc};"));
    }
    Some(lines)
}

/// GOW-Port: loop folding. The body of a block that branches back to its own start, as a function that runs up to `loopMax`
/// iterations of it (`<name>_loop`, registered beside the single-pass function). Returns `iterations << 32 | next pc`.
///
/// Why this is exact: the single-pass function and this one share every line of the body, so values, memory and flag-ring pushes
/// of an iteration are the same lines in the same order. What the runtime does between two iterations of the same block is
/// replaced by this function (`n.start` advances by the idle length `n.loopLen`, each iteration's tail record goes where the
/// history entry of that iteration would have it: `VU1NativeAccess::loopTail`) or by `VU1NativeAccess::foldIterations` afterwards
/// (history entries, FDIV timing, retire). Only blocks where nothing else can happen between two iterations are folded: no callout
/// (it commits pipelines and imports flags), no XGKICK / E / D / T bit (they leave the block), no read of flags written before the
/// block (`macAt` / `clipAt`: those need exact entry timing, not the idle table every iteration uses here), a branch to the block's
/// own first pair that is not a jump through a register, and no link register write.
fn gen_loop(pairs: &[Pair], body: &[String], has_tail: bool) -> Option<Vec<String>> {
    let n = pairs.len();
    if n < 2 {
        return None;
    }
    let (l, u) = pairs[n - 2];
    if !is_branch(l, u) || !matches!(l >> 25, 0x20 | 0x28 | 0x29 | 0x2C | 0x2D | 0x2E | 0x2F) {
        return None;
    }
    let target = (8 * (n as i64 - 2) + 8 + i64::from(sx(l, 11)) * 8).rem_euclid(0x4000);
    if target != 0 {
        return None;
    }
    if pairs.iter().any(|&(_, up)| (up >> 30) & 1 != 0 || (up >> 28) & 1 != 0 || (up >> 27) & 1 != 0) {
        return None;
    }
    let forbidden = ["VU1NativeAccess::call", "VU1NativeAccess::xgkick", "macAt(", "clipAt("];
    if body.iter().any(|line| forbidden.iter().any(|f| line.contains(f))) {
        return None;
    }
    let (last, rest) = body.split_last()?;
    let end_pc = last.strip_prefix("return taken ? target : ")?.strip_suffix(';')?;
    let mut out = vec!["uint32_t loopIt = 0;".to_owned(), "for (;;)".to_owned(), "{".to_owned()];
    if has_tail {
        out.push("    n.tail = VU1NativeAccess::loopTail(n, loopIt);".to_owned());
    }
    out.extend(rest.iter().map(|line| format!("    {line}")));
    out.push(format!("    const uint32_t loopResult = taken ? target : {end_pc};"));
    out.push("    ++loopIt;".to_owned());
    out.push("    if (loopResult != n.pc || loopIt >= loopMax)".to_owned());
    out.push("        return (static_cast<uint64_t>(loopIt) << 32) | loopResult;".to_owned());
    out.push("    n.start += n.loopLen;".to_owned());
    out.push("}".to_owned());
    Some(out)
}

/// Options of `vu1n-gen`.
pub struct Options {
    pub output: PathBuf,
    pub inputs: Vec<PathBuf>,
    pub heads: Option<PathBuf>,
    pub parts: usize,
    /// GOW-Port: the incremental cache (`<output>/.vu1n-cache` by default; `None` with `--no-cache`).
    pub cache: Option<PathBuf>,
}

struct BlockEntry {
    pc: u32,
    pairs: usize,
    body: Option<Vec<String>>,
    /// GOW-Port: the loop-folded variant of `body` (see `gen_loop`).
    loop_body: Option<Vec<String>>,
    tail: Option<Tail>,
}

// GOW-Port: incremental generation. A capture folder holds hundreds of thousands of micro-memory images (a player's had
// 484,087 for 2,026 distinct blocks) and reading them all took ~36 minutes on every build, almost all of it opening files.
// What an image contributes depends only on its bytes, its name (vu1s_*) and the extra heads, so the cache keeps the
// images already read (path, size, modification time) and, for every distinct block, where a full run would take it from:
// the first image in input order that has it, the pc there and its words. A later run reads only the new images and keeps
// that same "first image wins" rule, so its output is byte for byte the output of a full run (tests/tools.rs checks it).
// Any cached image missing or changed, other extra heads or another cache format: the cache is dropped and everything is read.
// The cache holds the game's VU1 code: it stays in the build folder like the generated sources (never shipped or published).
// Idea from gow-native-pc runtime patch 22 (VU1 analysis cache by image content, generator and heads).

/// Bump when what an image contributes changes (heads_of, extent, scan_heads, the key).
const CACHE_FORMAT: u32 = 4; // GOW-Port: 2 = no clamp after ADD / SUB / MUL, 3 = operands normalised once per block (planning 3.2), 4 = loop-folded variants of self-looping blocks

/// Where an image stands in input order: the index of the input argument, then the path (a folder's files sorted).
type Order = (u32, String);

#[derive(Clone)]
struct Found {
    order: Order,
    pc: u32,
    words: Vec<u32>,
}

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: u128,
}

struct Input {
    order: Order,
    path: PathBuf,
    stamp: Stamp,
}

fn stamp_of(metadata: &fs::Metadata) -> Stamp {
    let modified =
        metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    Stamp { size: metadata.len(), modified }
}

fn list_inputs(paths: &[PathBuf]) -> Result<Vec<Input>, String> {
    let mut inputs = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let arg = index as u32;
        if path.is_dir() {
            let mut files: Vec<Input> = Vec::new();
            for entry in fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?.filter_map(Result::ok) {
                let file = entry.path();
                if !file.extension().is_some_and(|ext| ext == "bin") {
                    continue;
                }
                // On Windows the directory listing already carries the size and time: no extra open per file.
                let metadata = entry.metadata().map_err(|e| format!("{}: {e}", file.display()))?;
                files.push(Input {
                    order: (arg, file.to_string_lossy().into_owned()),
                    path: file,
                    stamp: stamp_of(&metadata),
                });
            }
            files.sort_by(|a, b| a.path.cmp(&b.path));
            inputs.extend(files);
        } else {
            let metadata = fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
            inputs.push(Input {
                order: (arg, path.to_string_lossy().into_owned()),
                path: path.clone(),
                stamp: stamp_of(&metadata),
            });
        }
    }
    // The order of a folder's files is the order of their paths, as the PathBuf sort above; keep the strings in that order.
    Ok(inputs)
}

/// The blocks one image contributes: each distinct key once, at the lowest head pc that produces it.
fn image_blocks(path: &Path, extra: &BTreeSet<u32>) -> Result<Vec<(u64, u32, Vec<u32>)>, String> {
    let code = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if code.len() != CODE_SIZE {
        return Ok(Vec::new());
    }
    let mut heads = heads_of(&code, extra);
    if path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("vu1s_")) {
        // vu1s_*.bin holds code the ELF declares but no capture ever saw uploaded: nothing says where the game will
        // enter it, so every pair that is not empty is a possible block head.
        heads.extend(scan_heads(&code));
    }
    let mut seen: BTreeSet<u64> = BTreeSet::new();
    let mut out = Vec::new();
    for &pc in &heads {
        let pairs = extent(&code, pc as usize);
        if pairs.is_empty() {
            continue;
        }
        let words: Vec<u32> = pairs.iter().flat_map(|&(l, u)| [l, u]).collect();
        let key = fnv_block(&words);
        if seen.insert(key) {
            out.push((key, pc, words));
        }
    }
    Ok(out)
}

/// Keeps, for every key, the block of the earliest image in input order (what a full sequential run keeps).
fn merge_found(into: &mut BTreeMap<u64, Found>, key: u64, found: Found) {
    match into.get(&key) {
        Some(existing) if existing.order <= found.order => {}
        _ => {
            into.insert(key, found);
        }
    }
}

/// Reads `inputs` on several threads: opening files, not decoding, is what takes the time.
fn scan_images(inputs: &[&Input], extra: &BTreeSet<u32>) -> Result<BTreeMap<u64, Found>, String> {
    if inputs.is_empty() {
        return Ok(BTreeMap::new());
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 16).min(inputs.len());
    let chunk = inputs.len().div_ceil(threads);
    let results: Vec<Result<BTreeMap<u64, Found>, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = inputs
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    let mut local: BTreeMap<u64, Found> = BTreeMap::new();
                    for input in part {
                        for (key, pc, words) in image_blocks(&input.path, extra)? {
                            merge_found(&mut local, key, Found { order: input.order.clone(), pc, words });
                        }
                    }
                    Ok(local)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err("vu1n-gen: a reader thread panicked".to_owned())))
            .collect()
    });
    let mut merged: BTreeMap<u64, Found> = BTreeMap::new();
    for result in results {
        for (key, found) in result? {
            merge_found(&mut merged, key, found);
        }
    }
    Ok(merged)
}

fn heads_digest(extra: &BTreeSet<u32>) -> u64 {
    let words: Vec<u32> = extra.iter().copied().collect();
    fnv_block(&words)
}

struct Cache {
    files: std::collections::HashMap<String, Stamp>,
    blocks: BTreeMap<u64, Found>,
}

fn cache_header(digest: u64) -> String {
    format!("vu1n-cache {CACHE_FORMAT} {} {digest:016x}", env!("CARGO_PKG_VERSION"))
}

fn read_cache(path: &Path, digest: u64) -> Option<Cache> {
    let text = fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    if lines.next()? != cache_header(digest) {
        return None;
    }
    let mut cache = Cache { files: std::collections::HashMap::new(), blocks: BTreeMap::new() };
    for line in lines {
        let mut fields = line.split('\t');
        match fields.next()? {
            "F" => {
                let size = fields.next()?.parse().ok()?;
                let modified = fields.next()?.parse().ok()?;
                let name = fields.next()?.to_owned();
                cache.files.insert(name, Stamp { size, modified });
            }
            "B" => {
                let key = u64::from_str_radix(fields.next()?, 16).ok()?;
                let arg = fields.next()?.parse().ok()?;
                let pc = u32::from_str_radix(fields.next()?, 16).ok()?;
                let words =
                    fields.next()?.split(' ').map(|w| u32::from_str_radix(w, 16).ok()).collect::<Option<Vec<u32>>>()?;
                let name = fields.next()?.to_owned();
                cache.blocks.insert(key, Found { order: (arg, name), pc, words });
            }
            _ => return None,
        }
    }
    Some(cache)
}

fn write_cache(path: &Path, digest: u64, inputs: &[Input], blocks: &BTreeMap<u64, Found>) -> Result<(), String> {
    let mut text = String::with_capacity(inputs.len() * 80 + blocks.len() * 200);
    text.push_str(&cache_header(digest));
    text.push('\n');
    for input in inputs {
        text.push_str(&format!("F\t{}\t{}\t{}\n", input.stamp.size, input.stamp.modified, input.order.1));
    }
    for (key, found) in blocks {
        let words: Vec<String> = found.words.iter().map(|w| format!("{w:08x}")).collect();
        text.push_str(&format!(
            "B\t{key:016x}\t{}\t{:x}\t{}\t{}\n",
            found.order.0,
            found.pc,
            words.join(" "),
            found.order.1
        ));
    }
    // Written beside it and renamed: an interrupted build never leaves half a cache behind.
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Runs the generator; returns the summary line the Python tool printed.
pub fn run(options: &Options) -> Result<String, String> {
    let inputs = list_inputs(&options.inputs)?;

    let mut extra: BTreeSet<u32> = BTreeSet::new();
    if let Some(heads) = options.heads.as_ref().filter(|p| p.exists()) {
        for line in fs::read_to_string(heads).map_err(|e| format!("{}: {e}", heads.display()))?.lines() {
            let line = line.trim();
            if !line.is_empty() {
                extra.insert((u32::from_str_radix(line, 16).map_err(|_| format!("bad head: {line}"))?) & 0x3FF8);
            }
        }
    }

    let digest = heads_digest(&extra);
    // The cache is valid only if every image it read is still there, unchanged, at the same place in the input order.
    let cached = options.cache.as_deref().and_then(|path| read_cache(path, digest)).filter(|cache| {
        let current: std::collections::HashMap<&str, &Stamp> =
            inputs.iter().map(|i| (i.order.1.as_str(), &i.stamp)).collect();
        cache.files.iter().all(|(name, stamp)| current.get(name.as_str()).is_some_and(|s| *s == stamp))
            && cache.blocks.values().all(|f| inputs.iter().any(|i| i.order == f.order))
    });
    let (mut found, fresh): (BTreeMap<u64, Found>, Vec<&Input>) = match &cached {
        Some(cache) => {
            (cache.blocks.clone(), inputs.iter().filter(|i| !cache.files.contains_key(&i.order.1)).collect())
        }
        None => (BTreeMap::new(), inputs.iter().collect()),
    };
    for (key, block) in scan_images(&fresh, &extra)? {
        merge_found(&mut found, key, block);
    }

    let mut blocks: BTreeMap<u64, BlockEntry> = BTreeMap::new();
    let mut unsupported = 0usize;
    for (key, block) in &found {
        let pairs: Vec<Pair> = block.words.chunks(2).map(|w| (w[0], w[1])).collect();
        let mut tail = Vec::new();
        let body = gen_block(&pairs, &mut tail);
        if body.is_none() {
            unsupported += 1;
        }
        let tail = tail.into_iter().next();
        let loop_body = body.as_ref().and_then(|b| gen_loop(&pairs, b, tail.is_some()));
        blocks.insert(*key, BlockEntry { pc: block.pc, pairs: pairs.len(), body, loop_body, tail });
    }

    fs::create_dir_all(&options.output).map_err(|e| format!("{}: {e}", options.output.display()))?;
    let parts = options.parts.max(1);
    let mut texts: Vec<Vec<String>> = vec![Vec::new(); parts];
    let mut regs: Vec<Vec<String>> = vec![Vec::new(); parts];
    let mut count = 0usize;
    let mut folded = 0usize;
    for (key, entry) in &blocks {
        let Some(body) = &entry.body else {
            continue;
        };
        let name = format!("b{:04x}_{:08x}", entry.pc, key & 0xFFFF_FFFF);
        let part = count % parts;
        count += 1;
        let mut t: Vec<String> = Vec::new();
        if let Some((mac_pairs, mac_masks, clip_pairs)) = &entry.tail {
            let pad = |values: Vec<u64>| -> Vec<u64> {
                let mut v = values;
                v.resize(3, 0);
                v
            };
            let mp = pad(mac_pairs.iter().map(|&x| x as u64).collect());
            let mm = pad(mac_masks.iter().map(|&x| u64::from(x)).collect());
            let cp = pad(clip_pairs.iter().map(|&x| x as u64).collect());
            t.push(format!(
                "static const VU1TailDesc {name}_tail = {{{}, {}, {{{}, {}, {}}}, {{{}, {}, {}}}, {{{}, {}, {}}}}};",
                mac_pairs.len(),
                clip_pairs.len(),
                mp[0],
                mp[1],
                mp[2],
                mm[0],
                mm[1],
                mm[2],
                cp[0],
                cp[1],
                cp[2]
            ));
        }
        t.push(format!("static uint32_t {name}(VU1NativeCtx &n)"));
        t.push("{".to_owned());
        t.push("    VU1State &s = *n.s;".to_owned());
        t.extend(body.iter().map(|line| format!("    {line}")));
        t.push("}".to_owned());
        t.push(String::new());
        if let Some(loop_body) = &entry.loop_body {
            t.push(format!("static uint64_t {name}_loop(VU1NativeCtx &n, uint32_t loopMax)"));
            t.push("{".to_owned());
            t.push("    VU1State &s = *n.s;".to_owned());
            t.extend(loop_body.iter().map(|line| format!("    {line}")));
            t.push("}".to_owned());
            t.push(String::new());
            folded += 1;
        }
        texts[part].push(t.join("\n"));
        // Bit 16: the block reads flags written before it; only such blocks need exact entry timing (see runNative).
        let reads_entry = body.iter().any(|line| line.contains("macAt(") || line.contains("clipAt("));
        regs[part].push(format!(
            "    vu1nRegisterBlock(0x{key:016x}ull, {name}, {}u, {}{});",
            entry.pairs | if reads_entry { 0x10000 } else { 0 },
            if entry.tail.is_some() { format!("&{name}_tail") } else { "nullptr".to_owned() },
            if entry.loop_body.is_some() { format!(", {name}_loop") } else { String::new() }
        ));
    }

    let header = "// Generated by ps2-tools vu1n-gen from the game's VU1 microcode. Do not publish.\n#include \"runtime/ps2_vu1_native.h\"\n#include <cmath>\n\n#if defined(_MSC_VER)\n#pragma float_control(precise, on, push)\n#pragma fp_contract(off)\n#endif\n\n";
    let footer = "\n#if defined(_MSC_VER)\n#pragma float_control(pop)\n#endif\n";
    for k in 0..parts {
        let body = format!(
            "{header}{}\nnamespace\n{{\n    const bool g_registered = []\n    {{\n{}\n        return true;\n    }}();\n}}\n{footer}",
            texts[k].join("\n"),
            regs[k].join("\n")
        );
        let path = options.output.join(format!("vu1n_part{k}.cpp"));
        crate::write_if_changed(&path, &body).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    if let Some(path) = &options.cache {
        write_cache(path, digest, &inputs, &found)?;
    }
    let read =
        if cached.is_some() { format!(" ({} read, the rest from the cache)", fresh.len()) } else { String::new() };
    Ok(format!(
        "[vu1n_gen] {} images{read}, {} distinct blocks, {count} generated, {unsupported} unsupported, {folded} loop-folded",
        inputs.len(),
        blocks.len()
    ))
}
