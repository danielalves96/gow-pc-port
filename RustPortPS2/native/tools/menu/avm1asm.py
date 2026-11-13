"""AVM1 (Flash 5 style, no constant pool) disassembler/assembler with relocatable jumps.
Instr = [opcode, payload bytes, target_index_or_None]. `Jump` (0x99) and `If` (0x9D) hold a signed 16-bit offset
relative to the next instruction; on load they are turned into an instruction index so blocks can be inserted;
on assemble the offsets are recomputed.
"""
import struct

OP = {"Play": 0x06, "Stop": 0x07, "Add": 0x0A, "Subtract": 0x0B, "Divide": 0x0D, "Equals": 0x0E, "Less": 0x0F, "And": 0x10, "Or": 0x11, "Not": 0x12,
      "Pop": 0x17, "GetVariable": 0x1C, "SetVariable": 0x1D, "End": 0x00,
      "SetTarget": 0x8B, "GotoLabel": 0x8C, "Push": 0x96, "Jump": 0x99, "If": 0x9D, "Call": 0x9E, "GotoFrame": 0x81,
      "GotoFrame2": 0x9F}

class Ins:
    """target: for Jump/If, the Ins object jumped to (None = end of script)"""
    def __init__(self, op, payload=b"", target=None):
        self.op = op; self.payload = payload; self.target = target
    def __repr__(self):
        return "Ins(0x%02x, %r)" % (self.op, self.payload)

def load(blob):
    """blob -> list[Ins] (the terminating End instruction is kept)"""
    items = []; offs = []; i = 0
    while i < len(blob):
        op = blob[i]; offs.append(i)
        if op >= 0x80:
            n = struct.unpack_from("<H", blob, i + 1)[0]
            items.append(Ins(op, bytes(blob[i + 3:i + 3 + n]))); i += 3 + n
        else:
            items.append(Ins(op)); i += 1
    end_off = len(blob)
    for k, ins in enumerate(items):
        if ins.op in (0x99, 0x9D):
            rel = struct.unpack("<h", ins.payload)[0]
            tgt = offs[k] + 5 + rel      # next instruction address + rel (jump payload is 2 bytes: total 5)
            ins.target = None if tgt == end_off else items[offs.index(tgt)]
    return items

def assemble(items):
    offs = []; p = 0
    for ins in items:
        offs.append(p); p += 5 if ins.op in (0x99, 0x9D) else (3 + len(ins.payload) if ins.op >= 0x80 else 1)
    offs.append(p)
    where = {id(ins): offs[k] for k, ins in enumerate(items)}
    out = bytearray()
    for k, ins in enumerate(items):
        if ins.op in (0x99, 0x9D):
            rel = (offs[-1] if ins.target is None else where[id(ins.target)]) - offs[k + 1]
            out += bytes([ins.op]) + struct.pack("<Hh", 2, rel)
        elif ins.op >= 0x80:
            out += bytes([ins.op]) + struct.pack("<H", len(ins.payload)) + ins.payload
        else:
            out.append(ins.op)
    return bytes(out)

# ---- builders ----------------------------------------------------------------------------------------------------
def push_str(s): return Ins(0x96, b"\0" + s.encode() + b"\0")
def push_int(n): return Ins(0x96, b"\x07" + struct.pack("<i", n))
def push_num_str(n): return push_str(str(n))      # the game's scripts push numbers as strings ('1')
def set_target(s): return Ins(0x8B, s.encode() + b"\0")
def goto_label(s): return Ins(0x8C, s.encode() + b"\0")
def simple(name): return Ins(OP[name])

def text(items):
    names = {v: k for k, v in OP.items()}
    lines = []
    for k, ins in enumerate(items):
        t = ""
        if ins.op in (0x99, 0x9D): t = "-> %s" % ("END" if ins.target is None else items.index(ins.target))
        elif ins.op == 0x96:
            pl = ins.payload; j = 0; parts = []
            while j < len(pl):
                ty = pl[j]; j += 1
                if ty == 0:
                    e = pl.index(0, j); parts.append(repr(pl[j:e].decode("latin1"))); j = e + 1
                elif ty == 7: parts.append("i%d" % struct.unpack_from("<i", pl, j)[0]); j += 4
                else: parts.append("t%d?" % ty); break
            t = " ".join(parts)
        elif ins.op in (0x8B, 0x8C): t = repr(ins.payload.rstrip(b"\0").decode())
        lines.append("%4d %-12s %s" % (k, names.get(ins.op, hex(ins.op)), t))
    return "\n".join(lines)

def selftest(blobs):
    ok = 0
    for b in blobs:
        if assemble(load(b)) == b: ok += 1
    return ok, len(blobs)
