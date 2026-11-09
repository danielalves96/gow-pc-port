"""Helpers shared by the movie patch (object copies, AVM1 action cloning, and the lookups of the Options state machine)."""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import flp_model as fm
import avm1asm as A
from avm1asm import Ins

def need(c, msg):
    if not c: raise ValueError("unexpected movie shape: " + msg)

class Pool:
    def __init__(self, data): self.data = bytearray(data)
    def find(self, s):
        k = self.data.find(b"\0" + s.encode() + b"\0")
        need(k >= 0, "pool string " + s); return k + 1
    def add(self, s):
        off = len(self.data); self.data += s.encode() + b"\0"; return off
    def name(self, off):
        if off == 0xFFFF or off >= len(self.data): return None
        return self.data[off:self.data.index(0, off)].decode("latin1")

def copy_obj(o):
    n = fm.Obj18(); n.hdr = bytearray(o.hdr)
    n.layers = [[bytearray(r), [bytearray(e) for e in es]] for r, es in o.layers]
    n.frames = [[bytearray(r), [bytearray(a) for a in aents], list(bl)] for r, aents, bl in o.frames]
    return n

def clone(items, subst=lambda p: p):
    mp = {}; out = []
    for ins in items:
        c = Ins(ins.op, subst(ins.payload), ins.target); mp[id(ins)] = c; out.append(c)
    for c in out:
        if c.op in (0x99, 0x9D) and c.target is not None and id(c.target) in mp: c.target = mp[id(c.target)]
    return out

def is_push(ins, s): return ins.op == 0x96 and ins.payload == b"\0" + s.encode() + b"\0"

def find_state(items, k):
    hits = [i for i in range(len(items) - 3)
            if is_push(items[i], "PauseOptions_State") and items[i + 1].op == 0x1C and is_push(items[i + 2], str(k))
            and items[i + 3].op == 0x0E]
    need(len(hits) == 1, "state %d blocks: %d" % (k, len(hits)))
    return hits[0]

def cond(k, skip_to):
    c = [A.push_str("PauseOptions_State"), A.simple("GetVariable"), A.push_str(str(k)), A.simple("Equals"), A.simple("Not")]
    c.append(Ins(0x9D, b"", skip_to))
    return c

def label_step(path, label):
    return [A.set_target(path), A.goto_label(label), A.simple("Play"), A.set_target("")]

def set_state(k):
    return [A.push_str("PauseOptions_State"), A.push_str(str(k)), A.simple("SetVariable")]
