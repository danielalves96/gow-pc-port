"""Tree model of an FLP movie: parse -> edit -> serialize, following the loader's cursor (see flp.py).
All alignment padding in the retail files is zero, so the serializer pads with zeros; an unedited
parse+serialize is byte-identical to the input (checked by `selftest`).

Model (everything is kept as raw bytes except the lists that the loader walks):
  Movie.hdr        0x60 bytes (counts are recomputed on write; pointer fields are stale tool values the loader rewrites)
  Movie.t0         list of 4-byte entries           (character dictionary, {u16 type, u16 index})
  Movie.t1         list of [elem8, [child8 ...]]
  Movie.t2         list of T2 (font) records
  Movie.t3         list of [elem0x24, blob]          (static text)
  Movie.t4         list of 0x20-byte entries
  Movie.t5         list of T5 records
  Movie.t6         list of Obj18                     (clips)
  Movie.t7         Obj18                              (root timeline)
  Movie.t8         list of 0x14-byte entries          (placement transforms)
  Movie.t9         list of 8-byte entries             (placement color multipliers)
  Movie.t10        bytes                              (name pool)
"""
import struct, sys

def u16(b, o): return struct.unpack_from("<H", b, o)[0]
def u32(b, o): return struct.unpack_from("<I", b, o)[0]

class Obj18:
    """timeline object: 0x18 byte header, layers (8-byte record + 10-byte entries), frames (12-byte record + actions)"""
    def __init__(self):
        self.hdr = bytearray(0x18)
        self.layers = []   # [rec8 bytearray, [entry10 bytearray ...]]
        self.frames = []   # [rec12 bytearray, [bytes blob ...]]

class T2:
    def __init__(self):
        self.elem = bytearray(0x24)
        self.a = []        # [entry8, [child8...]]   (flags bit 1)
        self.b = []        # same                    (flags bit 2)
        self.u16s = b""
        self.blk = b""

class T5:
    def __init__(self):
        self.elem = bytearray(12)
        self.obj = Obj18()
        self.entries = []  # [entry16 bytearray, blob bytes]

class Reader:
    def __init__(self, d):
        self.d = d; self.cur = 0x60
    def align(self): self.cur = (self.cur + 3) & ~3
    def arr(self, n, size):
        """n elements of `size` bytes: one alignment for the whole array (like the loader)"""
        if n == 0: return []
        self.align()
        out = []
        for _ in range(n):
            b = self.d[self.cur:self.cur + size]
            if len(b) != size: raise ValueError("truncated")
            out.append(bytearray(b)); self.cur += size
        return out
    def take(self, n):
        self.align(); b = self.d[self.cur:self.cur + n]; self.cur += n
        if len(b) != n: raise ValueError("truncated")
        return bytearray(b)

def r_child8(r, elem):
    return r.arr(u16(elem, 2), 8) if False else None

def parse_obj18(r, hdr):
    o = Obj18(); o.hdr = hdr
    r.align()
    nl = u16(hdr, 2)
    recs = r.arr(nl, 8)
    for rec in recs:
        r.align()
        o.layers.append([rec, r.arr(u16(rec, 2), 10)])
    r.align()
    nf = u16(hdr, 4)
    frecs = r.arr(nf, 12)
    for rec in frecs:
        r.align()
        k = u16(rec, 2)
        aents = r.arr(k, 8)
        blobs = []
        for e in aents:
            r.align()
            blobs.append(bytes(r.take(u32(e, 0))))
        o.frames.append([rec, aents, blobs])
    return o

def parse(d):
    r = Reader(d)
    m = Movie()
    m.hdr = bytearray(d[:0x60])
    m.t0 = r.arr(u32(d, 0x0C), 4)
    t1e = r.arr(u32(d, 0x14), 8)
    for e in t1e:
        r.align()
        m.t1.append([e, r.arr(u16(e, 2), 8)])
    r.align()
    t2e = r.arr(u32(d, 0x1C), 0x24)
    for e in t2e:
        t = T2(); t.elem = e
        flags = u16(e, 0xC); n = u32(e, 0)
        for bit, lst in ((2, t.a), (4, t.b)):
            if flags & bit:
                ents = r.arr(n, 8)
                for x in ents:
                    r.align()
                    lst.append([x, r.arr(u16(x, 2), 8)])
        r.align()
        t.u16s = bytes(r.take(n * 2))
        r.align()
        t.blk = bytes(r.take(0x200 if flags & 1 else n * 2))
        r.align()
        m.t2.append(t)
    r.align()
    t3e = r.arr(u32(d, 0x24), 0x24)
    for e in t3e:
        r.align()
        m.t3.append([e, bytes(r.take(u32(e, 0x14)))])
    r.align()
    m.t4 = r.arr(u32(d, 0x2C), 0x20)
    r.align()
    t5e = r.arr(u32(d, 0x34), 12)
    for e in t5e:
        t = T5(); t.elem = e
        r.align()
        objhdr = r.take(0x18)
        t.obj = parse_obj18(r, objhdr)
        r.align()
        ents = r.arr(u16(e, 8), 16)
        for x in ents:
            r.align()
            t.entries.append([x, bytes(r.take(u32(x, 0)))])
        m.t5.append(t)
    r.align()
    t6h = r.arr(u32(d, 0x3C), 0x18)
    for h in t6h:
        m.t6.append(parse_obj18(r, h))
    r.align()
    m.t7 = parse_obj18(r, r.take(0x18))
    r.align()
    m.t8 = r.arr(u16(d, 0x48), 0x14)
    r.align()
    m.t9 = r.arr(u16(d, 0x50), 8)
    r.align()
    m.t10 = bytes(r.take(u16(d, 0x58)))
    if r.cur != len(d): raise ValueError("cursor %d != size %d" % (r.cur, len(d)))
    return m

class Movie:
    def __init__(self):
        self.hdr = bytearray(0x60)
        self.t0 = []; self.t1 = []; self.t2 = []; self.t3 = []; self.t4 = []; self.t5 = []; self.t6 = []
        self.t7 = Obj18(); self.t8 = []; self.t9 = []; self.t10 = b""

class Writer:
    def __init__(self):
        self.b = bytearray(b"\0" * 0x60)
        self.blob_log = []      # (list holding the blob, index, end offset) of every frame action blob written, in file order
    def align(self):
        while len(self.b) & 3: self.b.append(0)
    def put(self, data):
        self.align(); self.b += data
    def arr(self, items):
        """one alignment for the whole array, like the loader"""
        if not items: return
        self.align()
        for x in items: self.b += x

def fix_obj(o):
    o.hdr[2:4] = struct.pack("<H", len(o.layers)); o.hdr[4:6] = struct.pack("<H", len(o.frames))

def w_obj18(w, o):
    w.align()
    o.hdr[2:4] = struct.pack("<H", len(o.layers)); o.hdr[4:6] = struct.pack("<H", len(o.frames))
    for rec, ents in o.layers: rec[2:4] = struct.pack("<H", len(ents))
    w.arr([rec for rec, ents in o.layers])
    for rec, ents in o.layers:
        w.arr(ents)
    w.align()
    for rec, aents, blobs in o.frames: rec[2:4] = struct.pack("<H", len(aents))
    w.arr([rec for rec, aents, blobs in o.frames])
    for rec, aents, blobs in o.frames:
        for e, bl in zip(aents, blobs): e[0:4] = struct.pack("<I", len(bl))
        w.arr(aents)
        for i, bl in enumerate(blobs):
            w.align(); w.b += bl
            w.blob_log.append((blobs, i, len(w.b)))

def serialize(m, drop_t9=False, drop_t8=False, window=None):
    """window = (begin, end): a byte range of the output that must hold nothing live. The HUD movie is parsed in the buffer the game loaded
    the retail one into, and the game's heap writes its own bookkeeping (block pointers) into [0x1FF30, 0x20000) of that buffer. The retail
    movie has dead records there; a rebuilt one has whatever its layout puts there, and a record the loader needs gets corrupted (the
    menu then crashes or stays black, depending on the exact size of the movie). The range is made dead padding: zero bytes after the End
    action of the last frame action blob that ends before it (the interpreter never reaches them), enough to cover it."""
    if window is None:
        return _serialize(m, drop_t9, drop_t8, None)
    lo, hi = window
    w = Writer()
    _serialize(m, drop_t9, drop_t8, w)
    ends = [(end, blobs, i) for blobs, i, end in w.blob_log if end <= lo]
    if not ends:
        raise ValueError("no action blob ends before the volatile window %#x" % lo)
    end, blobs, i = max(ends, key=lambda e: e[0])
    pad = (hi - end + 3) & ~3
    blobs[i] = bytes(blobs[i]) + bytes(pad)
    check = Writer()
    out = _serialize(m, drop_t9, drop_t8, check)
    end2 = next(e for b, k, e in check.blob_log if b is blobs and k == i)
    if not (end2 - pad <= lo and end2 >= hi):
        raise ValueError("the volatile window is not inside the padding (%#x..%#x, window %#x..%#x)" % (end2 - pad, end2, lo, hi))
    return out

def _serialize(m, drop_t9, drop_t8, w):
    """drop_t9 / drop_t8: the read-only tables T9 (8-byte color multipliers) and T8 (20-byte transforms) are left out and their
    header counts are written as 0; the runtime supplies them from external blocks and fixes the pointers (@0x4C, @0x54) and counts
    after the loader's in-place parse (keeps the movie inside the original buffer)."""
    if w is None:
        w = Writer()
    h = m.hdr
    def setc(off, v, wide=4): h[off:off + wide] = struct.pack("<I" if wide == 4 else "<H", v)
    setc(0x0C, len(m.t0)); setc(0x14, len(m.t1)); setc(0x1C, len(m.t2)); setc(0x24, len(m.t3)); setc(0x2C, len(m.t4))
    setc(0x34, len(m.t5)); setc(0x3C, len(m.t6)); setc(0x48, 0 if drop_t8 else len(m.t8), 2); setc(0x50, 0 if drop_t9 else len(m.t9), 2); setc(0x58, len(m.t10), 2)
    w.arr(m.t0)
    for e, ch in m.t1: e[2:4] = struct.pack("<H", len(ch))
    w.arr([e for e, ch in m.t1])
    for e, ch in m.t1:
        w.arr(ch)
    w.align()
    w.arr([t.elem for t in m.t2])
    for t in m.t2:
        flags = u16(t.elem, 0xC); n = u32(t.elem, 0)
        for bit, lst in ((2, t.a), (4, t.b)):
            if flags & bit:
                w.arr([x for x, ch in lst])
                for x, ch in lst:
                    w.arr(ch)
        w.align(); w.b += t.u16s
        w.align(); w.b += t.blk
        w.align()
    w.align()
    for e, bl in m.t3: e[0x14:0x18] = struct.pack("<I", len(bl))
    w.arr([e for e, bl in m.t3])
    for e, bl in m.t3:
        w.align(); w.b += bl
    w.align()
    w.arr(m.t4)
    w.align()
    for t in m.t5: t.elem[8:10] = struct.pack("<H", len(t.entries))
    w.arr([t.elem for t in m.t5])
    for t in m.t5:
        fix_obj(t.obj)
        w.align(); w.b += t.obj.hdr  # obj header reserved at the cursor
        w_obj18(w, t.obj)
        w.align()
        for x, bl in t.entries: x[0:4] = struct.pack("<I", len(bl))
        w.arr([x for x, bl in t.entries])
        for x, bl in t.entries:
            w.align(); w.b += bl
    w.align()
    for o in m.t6:
        o.hdr[2:4] = struct.pack("<H", len(o.layers)); o.hdr[4:6] = struct.pack("<H", len(o.frames))
    w.arr([o.hdr for o in m.t6])
    for o in m.t6: w_obj18(w, o)
    fix_obj(m.t7)
    w.align(); w.b += m.t7.hdr
    w_obj18(w, m.t7)
    w.align()
    if not drop_t8:
        w.arr(m.t8)
    w.align()
    if not drop_t9:
        w.arr(m.t9)
    w.align()
    w.b += m.t10
    out = w.b
    out[0:0x60] = h
    return bytes(out)

def selftest(path):
    d = open(path, "rb").read()
    m = parse(d)
    s = serialize(m)
    ok = s == d
    print("%s: %d bytes, rebuild %s" % (path, len(d), "IDENTICAL" if ok else "DIFFERENT (first diff at %s)" % next((i for i in range(min(len(s), len(d))) if s[i] != d[i]), None)))
    return ok

if __name__ == "__main__":
    for p in sys.argv[1:]: selftest(p)
