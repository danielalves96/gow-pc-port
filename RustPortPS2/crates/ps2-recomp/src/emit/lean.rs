//! GOW-Port: `[codegen] lean_pc` - which statements can go without their `ctx->pc` store.
//!
//! The classic output stores the guest pc before every instruction and four to six times around every branch. Only
//! the runtime reads it (device accesses, syscalls, exceptions, a checkpoint's resume), so a statement that cannot
//! reach the runtime does not need it. Idea and numbers from gta-vcs-recomp (`pc_observation.h`, its
//! docs/PERFORMANCE.md section 2: 5-10 % of the game thread).

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

/// R5900 instructions that neither trap nor call into the runtime: plain register arithmetic, moves and logic. ADD /
/// SUB / DADD / DSUB (overflow traps), SYSCALL, BREAK, SYNC and the trap instructions are not here.
pub fn cannot_observe_pc(inst: &Instruction) -> bool {
    if inst.is_load || inst.is_store || inst.is_mmio || inst.is_branch || inst.is_jump || inst.has_delay_slot {
        return false;
    }
    match inst.opcode {
        OPCODE_SPECIAL => matches!(
            inst.function,
            0x00 | 0x02 | 0x03 | 0x04 | 0x06 | 0x07 // shifts
                | 0x0A | 0x0B // MOVZ / MOVN
                | 0x10..=0x13 // MFHI MTHI MFLO MTLO
                | 0x14 | 0x16 | 0x17 // DSLLV DSRLV DSRAV
                | 0x18..=0x1B // MULT MULTU DIV DIVU (no traps on the R5900)
                | 0x21 | 0x23..=0x27 // ADDU SUBU AND OR XOR NOR
                | 0x28 | 0x29 | 0x2A | 0x2B | 0x2D | 0x2F // MFSA MTSA SLT SLTU DADDU DSUBU
                | 0x38 | 0x3A | 0x3B | 0x3C | 0x3E | 0x3F // DSLL DSRL DSRA DSLL32 DSRL32 DSRA32
        ),
        0x09..=0x0F => true, // ADDIU SLTI SLTIU ANDI ORI XORI LUI
        OPCODE_DADDIU | OPCODE_MMI | OPCODE_COP1 => true,
        _ => false,
    }
}

/// A RAM access the generated code does in place: its translation goes through the READ* / WRITE* macros, whose slow
/// path stores `ps2AccessPc` before a device access, or through FAST_* for a constant RAM address.
fn is_plain_memory(inst: &Instruction) -> bool {
    (inst.is_load || inst.is_store) && !inst.is_mmio
}

/// Text that reaches the runtime, reads the pc itself, or leaves the function: such a statement keeps its pc store.
fn reaches_runtime(text: &str) -> bool {
    ["runtime", "ctx->pc", "throw", "ps2_syscalls", "ps2_stubs", "return", "goto"]
        .iter()
        .any(|word| text.contains(word))
}

/// Uses a checked guest access macro (not only the constant-address FAST_* forms).
fn uses_checked_access(text: &str) -> bool {
    let rest = text.replace("FAST_READ", "").replace("FAST_WRITE", "");
    rest.contains("READ") || rest.contains("WRITE")
}

/// The statement for `inst` without a pc store, or `None` when it must keep it. `translation` is the translated
/// instruction alone (no comment, no pc store).
pub fn lean_statement(inst: &Instruction, translation: &str) -> Option<String> {
    if reaches_runtime(translation) {
        return None;
    }
    if is_plain_memory(inst) {
        if uses_checked_access(translation) {
            return Some(format!(
                "{{ constexpr uint32_t ps2AccessPc = 0x{:x}u; {} }}",
                inst.address,
                translation.trim_end()
            ));
        }
        return Some(translation.to_owned());
    }
    cannot_observe_pc(inst).then(|| translation.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(raw: u32) -> Instruction {
        ps2_r5900::decode(0x100000, raw, false)
    }

    #[test]
    fn pure_arithmetic_drops_its_pc_store() {
        // addiu $sp, $sp, -0x100
        let inst = decode(0x27bd_ff00);
        assert_eq!(lean_statement(&inst, "SET_GPR_S32(ctx, 29, 1);").as_deref(), Some("SET_GPR_S32(ctx, 29, 1);"));
    }

    #[test]
    fn a_ram_access_passes_its_pc_to_the_slow_path() {
        // lw $a1, -0x4218($s2)
        let inst = decode(0x8e45_bde8);
        let lean = lean_statement(&inst, "SET_GPR_S32(ctx, 5, (int32_t)READ32(ADD32(GPR_U32(ctx, 18), 1)));").unwrap();
        assert!(lean.starts_with("{ constexpr uint32_t ps2AccessPc = 0x100000u; "), "{lean}");
        let constant = lean_statement(&inst, "SET_GPR_S32(ctx, 5, (int32_t)FAST_READ32(0x29BDE8u));").unwrap();
        assert!(!constant.contains("ps2AccessPc"), "a constant RAM address has no slow path: {constant}");
    }

    #[test]
    fn runtime_calls_and_traps_keep_it() {
        let lw = decode(0x8e45_bde8);
        assert!(lean_statement(&lw, "SET_GPR_S32(ctx, 5, runtime->Load32(rdram, ctx, 0x10003000u));").is_none());
        // add $v0, $a0, $a1 (overflow trap)
        let add = decode(0x0085_1020);
        assert!(lean_statement(&add, "SET_GPR_S32(ctx, 2, 0);").is_none());
        // syscall
        let syscall = decode(0x0000_000c);
        assert!(lean_statement(&syscall, "SET_GPR_S32(ctx, 2, 0);").is_none());
    }
}
