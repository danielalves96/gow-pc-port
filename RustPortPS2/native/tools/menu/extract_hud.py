#!/usr/bin/env python3
"""Finds the FLP_HUD movie (or the one named by the third argument, e.g. FLP_Shell: the title menu) in the retail PAK (the user's
own copy) and writes it out.

usage: extract_hud.py <PART1.PAK> <FLP_HUD.bin> [FLP_Shell]

The PAK holds WAD records of [u32 tag][u32 size][char name[24]][data]; the record named FLP_HUD is the HUD and menu movie
(479,303 bytes in the retail game), FLP_Shell the title menu (166,972 bytes). The output is a working file for make_patch.py: never
commit it.
"""
import struct, sys

def main():
    pak = open(sys.argv[1], "rb").read()
    name = sys.argv[3] if len(sys.argv) > 3 else "FLP_HUD"
    key = name.encode() + b"\0"
    hits = []
    k = pak.find(key)
    while k >= 0:
        rec = k - 8
        if rec >= 0 and pak[rec + 8:rec + 32].split(b"\0")[0] == name.encode():
            size = struct.unpack_from("<I", pak, rec + 4)[0]
            if 0x60 < size < 0x2000000 and rec + 32 + size <= len(pak):
                hits.append((rec, size))
        k = pak.find(key, k + 1)
    data = {pak[r + 32:r + 32 + s] for r, s in hits}
    if len(data) != 1:
        sys.exit("expected one %s movie in %s, found %d (%d records)" % (name, sys.argv[1], len(data), len(hits)))
    out = next(iter(data))
    open(sys.argv[2], "wb").write(out)
    print("%s: %d bytes (record at 0x%x)" % (name, len(out), hits[0][0]))

main()
