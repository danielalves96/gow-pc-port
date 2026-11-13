//! GOW-Port: `[codegen] ram_groups` - one RAM range check for several accesses off the same base register.
//!
//! Every guest load and store of the "gow" output checks its own address (`READ32` / `WRITE32`: mask, compare against the
//! RAM size, out-of-line slow path). A run of straight-line instructions that accesses `0x10($s0)`, `0x14($s0)`,
//! `0x20($s0)` with `$s0` unchanged between them checks the same thing three times. Here the first access of such a
//! run computes the check once for all of them,
//!
//! ```text
//! { const uint32_t a = ADD32(GPR_U32(ctx, 16), omin); gm = a & 0xDFFFFFFFu; gf = gm < PS2_FAST_RAM_LIMIT - dmax; }
//! ```
//!
//! and each member becomes `GREAD32(gf, gm, d, <the original address>)` / `GWRITE32(gf, gm, d, <address>, <value>)`: when
//! `gf` is set the access is a plain load or store at `rdram + gm + d`, otherwise it is the original macro, so
//! scratchpad, I/O, VU memory and the last 16 bytes of RAM behave as before.
//!
//! Why the fast path is exact. The original test of one access is `(addr & 0xDFFFFFFF) < PS2_FAST_RAM_LIMIT` with
//! `addr = base + o_i` (32-bit wrap) and the RAM offset is that masked value. With `a = base + omin`, `gm = a &
//! 0xDFFFFFFF` and `gm + dmax < PS2_FAST_RAM_LIMIT` (< 2^25), every `a + d` (`d = o_i - omin`, 0 <= d <= dmax) only
//! changes the bits below 2^25 of `a`, whose upper bits are 0 or 0x20000000: `(a + d) & 0xDFFFFFFF == gm + d`, which is
//! below the limit, so the per-access test would have passed with that very offset. For the 128-bit accesses the
//! original aligns first: `((a + d) & ~0xF) & 0xDFFFFFFF == (gm + d) & ~0xF` for the same reason.
//!
//! What may sit between the first and the last member: anything that neither writes the base register nor can reach
//! the runtime (a call, a syscall, a device access, an exception) nor jumps. A group is closed there, at every
//! instruction that writes its base register (after the member that does, when it is one), at every branch, and at every
//! label that a `goto` can reach. The labels that only the entry `switch (ctx->pc)` reaches (resume points, the unresolved
//! jr / jalr fallback that labels every instruction) stay inside groups: the flags are locals that start false, so an
//! entry in the middle of a group runs its members through the original macros.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::emit::locals::{Chunk, ChunkKind};

/// Widths of the guest access macros (`READ8` ... `READ128`).
const WIDTHS: [u32; 5] = [8, 16, 32, 64, 128];

/// Text of a translation that stops a group: it can reach the runtime or leave the function, or it writes a register
/// the scan for `SET_GPR_*` does not see.
const CLOSES_ALL: [&str; 8] = ["runtime", "throw", "return", "goto", "ctx->pc", "ps2PmulthExec", "->r[", "setReg"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
}

#[derive(Debug)]
struct Member {
    chunk: usize,
    address: u32,
    offset: i32,
    access: Access,
    width: u32,
    /// `ADD32(GPR_U32(ctx, rs), simm)`: the address expression as the translator wrote it.
    addr: String,
}

#[derive(Debug)]
struct Open {
    base: u32,
    /// The chunk reserved for the check, in front of the first member.
    setup: usize,
    members: Vec<Member>,
}

/// The finished groups' address ranges, to check that no `goto` lands inside one.
#[derive(Debug, Clone, Copy)]
struct Span {
    first: u32,
    last: u32,
}

#[derive(Debug, Default)]
pub struct RamGroups {
    open: Vec<Option<Open>>,
    finished: usize,
    decls: String,
    spans: Vec<Span>,
}

impl RamGroups {
    pub fn new() -> Self {
        Self { open: (0..32).map(|_| None).collect(), ..Self::default() }
    }

    /// The declarations of the group flags; they go before the entry `switch` (a `goto` may not jump over an initializer).
    pub fn declarations(&self) -> &str {
        &self.decls
    }

    /// The instruction whose statement the caller pushes as the next chunk: `translation` is its translated text.
    pub fn observe(&mut self, inst: &Instruction, translation: &str, chunks: &mut Vec<Chunk>) {
        if CLOSES_ALL.iter().any(|word| translation.contains(word)) {
            self.close_all(chunks);
            return;
        }

        if let Some((access, width, addr)) = member_pattern(inst, translation) {
            let base = inst.rs as usize;
            if self.open[base].is_none() {
                let setup = chunks.len();
                chunks.push(Chunk { kind: ChunkKind::Other, text: String::new() });
                self.open[base] = Some(Open { base: inst.rs, setup, members: Vec::new() });
            }
            if let Some(group) = self.open[base].as_mut() {
                group.members.push(Member {
                    chunk: chunks.len(),
                    address: inst.address,
                    offset: inst.simmediate as i32,
                    access,
                    width,
                    addr,
                });
            }
        }
    }

    /// After the caller pushed the statement chunk of the instruction `observe` saw: the groups whose base register it
    /// writes end here (the instruction itself may be a member of one: it addressed with the old value).
    pub fn settle(&mut self, translation: &str, chunks: &mut [Chunk]) {
        match written_registers(translation) {
            Some(registers) => {
                for register in registers {
                    self.close(register as usize, chunks);
                }
            }
            None => self.close_all(chunks),
        }
    }

    /// A branch, a label a `goto` can reach, a GIF DMA kick...: nothing carries over.
    pub fn close_all(&mut self, chunks: &mut [Chunk]) {
        for register in 1..self.open.len() {
            self.close(register, chunks);
        }
    }

    fn close(&mut self, register: usize, chunks: &mut [Chunk]) {
        let Some(group) = self.open.get_mut(register).and_then(Option::take) else {
            return;
        };
        if group.members.len() < 2 {
            return;
        }
        let id = self.finished;
        self.finished += 1;

        let omin = group.members.iter().map(|m| m.offset).min().unwrap_or(0);
        let dmax = group.members.iter().map(|m| i64::from(m.offset) - i64::from(omin)).max().unwrap_or(0);
        let flag = format!("ps2g{id}f");
        let mask = format!("ps2g{id}m");

        let _ = writeln!(self.decls, "    bool {flag} = false;\n    uint32_t {mask} = 0u;");
        chunks[group.setup].text = format!(
            "    {{ const uint32_t ps2ga = ADD32(GPR_U32(ctx, {}), {}u); {mask} = ps2ga & 0xDFFFFFFFu; \
             {flag} = {mask} < (PS2_FAST_RAM_LIMIT - {dmax}u); }}\n",
            group.base, omin as u32
        );

        for member in &group.members {
            let d = i64::from(member.offset) - i64::from(omin);
            let text = &mut chunks[member.chunk].text;
            let (old, new) = match member.access {
                Access::Read => (
                    format!("READ{}({}", member.width, member.addr),
                    format!("GREAD{}({flag}, {mask}, {d}u, {}", member.width, member.addr),
                ),
                Access::Write => (
                    format!("WRITE{}({}, ", member.width, member.addr),
                    format!("GWRITE{}({flag}, {mask}, {d}u, {}, ", member.width, member.addr),
                ),
            };
            *text = text.replacen(&old, &new, 1);
        }
        let first = group.members.iter().map(|m| m.address).min().unwrap_or(0);
        let last = group.members.iter().map(|m| m.address).max().unwrap_or(0);
        self.spans.push(Span { first, last });
    }

    /// The `goto` targets of the emitted body that fall inside a group: the groups are closed at every label a `goto`
    /// can reach, so there must be none. Returns the first such `(target, group start, group end)`.
    pub fn goto_into_a_group(&self, body: &[Chunk]) -> Option<(u32, u32, u32)> {
        for chunk in body {
            let mut rest = chunk.text.as_str();
            while let Some(at) = rest.find("goto label_") {
                rest = &rest[at + "goto label_".len()..];
                let digits: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
                if let Ok(target) = u32::from_str_radix(&digits, 16) {
                    if let Some(span) = self.spans.iter().find(|s| target > s.first && target <= s.last) {
                        return Some((target, span.first, span.last));
                    }
                }
            }
        }
        None
    }
}

/// `READ<w>(addr)` or `WRITE<w>(addr, ...)` with the plain address expression of this instruction, once in `translation`.
fn member_pattern(inst: &Instruction, translation: &str) -> Option<(Access, u32, String)> {
    if inst.rs == 0 || !(inst.is_load || inst.is_store) || inst.is_mmio {
        return None;
    }
    let addr = format!("ADD32(GPR_U32(ctx, {}), {})", inst.rs, inst.simmediate);
    let mut found = None;
    let mut count = 0;
    for width in WIDTHS {
        let read = format!("READ{width}({addr}");
        let write = format!("WRITE{width}({addr}, ");
        let reads = count_standalone(translation, &read);
        let writes = count_standalone(translation, &write);
        count += reads + writes;
        if reads == 1 {
            found = Some((Access::Read, width, addr.clone()));
        }
        if writes == 1 {
            found = Some((Access::Write, width, addr.clone()));
        }
    }
    if count == 1 {
        found
    } else {
        None
    }
}

/// Occurrences of `pattern` not preceded by an identifier character (`READ32(` but not `FAST_READ32(`).
fn count_standalone(text: &str, pattern: &str) -> usize {
    let bytes = text.as_bytes();
    text.match_indices(pattern)
        .filter(|(at, _)| *at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_'))
        .count()
}

/// The GPRs the translation writes (`SET_GPR_*(ctx, N, ...)`), or `None` when it writes one the scan cannot name.
fn written_registers(translation: &str) -> Option<BTreeSet<u32>> {
    let mut registers = BTreeSet::new();
    let mut rest = translation;
    while let Some(at) = rest.find("SET_GPR_") {
        rest = &rest[at..];
        let open = rest.find('(')?;
        let args = rest[open + 1..].trim_start();
        let args = args.strip_prefix("ctx,")?.trim_start();
        let digits: String = args.chars().take_while(char::is_ascii_digit).collect();
        registers.insert(digits.parse().ok()?);
        rest = &rest[open + 1..];
    }
    Some(registers)
}

/// The addresses a `goto` of the emitted body can name: branch and jump targets inside the function and the targets of
/// the jump tables. (Resume points and the unresolved-jump fallback labels are reached by the entry switch only.)
pub fn goto_targets(instructions: &[Instruction], jump_table_targets: impl Iterator<Item = u32>) -> BTreeSet<u32> {
    let mut targets: BTreeSet<u32> = jump_table_targets.collect();
    for inst in instructions {
        if inst.opcode == OPCODE_J || inst.opcode == OPCODE_JAL {
            targets.insert(crate::translate::instruction::build_absolute_jump_target(inst.address, inst.target));
        } else if inst.is_branch {
            let offset = i32::from(inst.simmediate as u16 as i16) << 2;
            targets.insert(inst.address.wrapping_add(4).wrapping_add(offset as u32));
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(raw: u32) -> Instruction {
        ps2_r5900::decode(0x100000, raw, false)
    }

    fn lw(rt: u32, base: u32, offset: i16) -> u32 {
        (0x23 << 26) | (base << 21) | (rt << 16) | u32::from(offset as u16)
    }

    fn sw(rt: u32, base: u32, offset: i16) -> u32 {
        (0x2b << 26) | (base << 21) | (rt << 16) | u32::from(offset as u16)
    }

    fn chunk(text: &str) -> Chunk {
        Chunk { kind: ChunkKind::Instruction, text: text.to_owned() }
    }

    /// What `emit_function` does for a plain load.
    fn load(groups: &mut RamGroups, chunks: &mut Vec<Chunk>, rt: u32, base: u32, offset: i16) {
        let inst = decode(lw(rt, base, offset));
        let translation =
            format!("SET_GPR_S32(ctx, {rt}, (int32_t)READ32(ADD32(GPR_U32(ctx, {base}), {})));", inst.simmediate);
        groups.observe(&inst, &translation, chunks);
        chunks.push(chunk(&format!("    {translation}\n")));
        groups.settle(&translation, chunks);
    }

    fn store(groups: &mut RamGroups, chunks: &mut Vec<Chunk>, rt: u32, base: u32, offset: i16) {
        let inst = decode(sw(rt, base, offset));
        let translation = format!("WRITE32(ADD32(GPR_U32(ctx, {base}), {}), GPR_U32(ctx, {rt}));", inst.simmediate);
        groups.observe(&inst, &translation, chunks);
        chunks.push(chunk(&format!("    {translation}\n")));
        groups.settle(&translation, chunks);
    }

    #[test]
    fn two_accesses_off_one_base_share_a_check() {
        let mut groups = RamGroups::new();
        let mut chunks = Vec::new();
        load(&mut groups, &mut chunks, 2, 16, 0x10);
        store(&mut groups, &mut chunks, 3, 16, 0x24);
        groups.close_all(&mut chunks);

        let text: String = chunks.iter().map(|c| c.text.as_str()).collect();
        assert!(text.contains("ps2g0m = ps2ga & 0xDFFFFFFFu; ps2g0f = ps2g0m < (PS2_FAST_RAM_LIMIT - 20u);"), "{text}");
        assert!(text.contains("ADD32(GPR_U32(ctx, 16), 16u)"), "the group starts at the lowest offset: {text}");
        assert!(text.contains("GREAD32(ps2g0f, ps2g0m, 0u, ADD32(GPR_U32(ctx, 16), 16))"), "{text}");
        assert!(text.contains("GWRITE32(ps2g0f, ps2g0m, 20u, ADD32(GPR_U32(ctx, 16), 36), GPR_U32(ctx, 3))"), "{text}");
        assert!(groups.declarations().contains("bool ps2g0f = false;"));
    }

    #[test]
    fn a_lone_access_keeps_its_own_check() {
        let mut groups = RamGroups::new();
        let mut chunks = Vec::new();
        load(&mut groups, &mut chunks, 2, 16, 0x10);
        groups.close_all(&mut chunks);
        let text: String = chunks.iter().map(|c| c.text.as_str()).collect();
        assert!(!text.contains("GREAD") && groups.declarations().is_empty(), "{text}");
    }

    #[test]
    fn a_write_to_the_base_closes_the_group_after_the_member_that_writes_it() {
        let mut groups = RamGroups::new();
        let mut chunks = Vec::new();
        load(&mut groups, &mut chunks, 4, 16, 0); // $a0 = 0($s0)
        load(&mut groups, &mut chunks, 16, 16, 8); // $s0 = 8($s0): the old $s0 still addresses it
        load(&mut groups, &mut chunks, 5, 16, 12); // the new $s0: not in the group
        load(&mut groups, &mut chunks, 6, 16, 16);
        groups.close_all(&mut chunks);

        let text: String = chunks.iter().map(|c| c.text.as_str()).collect();
        assert!(text.contains("GREAD32(ps2g0f, ps2g0m, 0u,") && text.contains("GREAD32(ps2g0f, ps2g0m, 8u,"), "{text}");
        assert!(text.contains("GREAD32(ps2g1f, ps2g1m, 0u,") && text.contains("GREAD32(ps2g1f, ps2g1m, 4u,"), "{text}");
        assert!(text.contains("PS2_FAST_RAM_LIMIT - 8u") && text.contains("PS2_FAST_RAM_LIMIT - 4u"), "{text}");
    }

    #[test]
    fn a_runtime_call_closes_everything() {
        let mut groups = RamGroups::new();
        let mut chunks = Vec::new();
        load(&mut groups, &mut chunks, 2, 16, 0);
        let syscall = decode(0x0000_000c);
        groups.observe(&syscall, "runtime->handleSyscall(rdram, ctx);", &mut chunks);
        chunks.push(chunk("    runtime->handleSyscall(rdram, ctx);\n"));
        groups.settle("runtime->handleSyscall(rdram, ctx);", &mut chunks);
        load(&mut groups, &mut chunks, 3, 16, 4);
        groups.close_all(&mut chunks);
        let text: String = chunks.iter().map(|c| c.text.as_str()).collect();
        assert!(!text.contains("GREAD"), "one access on each side of a call: {text}");
    }

    #[test]
    fn a_goto_into_a_group_is_found() {
        let mut groups = RamGroups::new();
        let mut chunks = Vec::new();
        load(&mut groups, &mut chunks, 2, 16, 0);
        load(&mut groups, &mut chunks, 3, 16, 4);
        load(&mut groups, &mut chunks, 4, 16, 8);
        groups.close_all(&mut chunks);
        // the members sit at 0x100000 for all three here: widen the span by hand
        groups.spans[0] = Span { first: 0x100000, last: 0x100008 };
        let body = [chunk("    goto label_100004;\n")];
        assert_eq!(groups.goto_into_a_group(&body), Some((0x100004, 0x100000, 0x100008)));
        let outside = [chunk("    goto label_100000;\n    goto label_100020;\n")];
        assert_eq!(groups.goto_into_a_group(&outside), None);
    }

    #[test]
    fn written_registers_reads_the_destination() {
        let regs = written_registers("SET_GPR_S32(ctx, 17, (int32_t)READ32(x)); SET_GPR_VEC(ctx, 5, y);").unwrap();
        assert_eq!(regs.into_iter().collect::<Vec<_>>(), vec![5, 17]);
        assert!(written_registers("WRITE32(a, GPR_U32(ctx, 3));").unwrap().is_empty());
    }

    #[test]
    fn goto_targets_cover_branches_jumps_and_tables() {
        // beq $zero, $zero, +2 at 0x100000 -> 0x10000c; j 0x400
        let beq = decode(0x1000_0002);
        let j = ps2_r5900::decode(0x100010, (0x02 << 26) | (0x400 >> 2), false);
        let targets = goto_targets(&[beq, j], [0x100100u32].into_iter());
        assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec![0x400, 0x10000c, 0x100100]);
    }
}
