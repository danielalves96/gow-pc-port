#!/usr/bin/env python3
"""Regenerates src/game/gow_menu_patch.h (Port Options in the pause menu) and gow_menu_patch_shell.h (Port Options in the title menu)
from the user's own retail game.

usage: make_patch.py <PART1.PAK or an extracted FLP_HUD.bin> [work folder]   (the title menu needs the PAK)

1. extracts the FLP_HUD movie (extract_hud.py), 2. builds the patched movie (flp_patch_cheats.py), 3. writes the delta that turns
the retail movie into the patched one (make_delta.py). Only the header is meant to be committed; the movies are game files.
"""
import os, subprocess, sys

here = os.path.dirname(os.path.abspath(__file__))
header = os.path.normpath(os.path.join(here, "..", "..", "ps2xRuntime", "src", "game", "gow_menu_patch.h"))
header_shell = os.path.normpath(os.path.join(here, "..", "..", "ps2xRuntime", "src", "game", "gow_menu_patch_shell.h"))

def run(*args):
    print("+", " ".join(args)); subprocess.run([sys.executable, "-I", *args], check=True)

def main():
    src = sys.argv[1]
    # default: the repository's git-ignored build-gen/ (the movies are game files and must never be committed)
    work = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "..", "..", "..", "..", "build-gen", "menu-patch")
    os.makedirs(work, exist_ok=True)
    hud = os.path.join(work, "FLP_HUD.bin")
    if src.lower().endswith(".pak"):
        run(os.path.join(here, "extract_hud.py"), src, hud)
    else:
        open(hud, "wb").write(open(src, "rb").read())
    patched = os.path.join(work, "FLP_HUD.cheats.bin")
    run(os.path.join(here, "flp_patch_cheats.py"), hud, patched)
    run(os.path.join(here, "make_delta.py"), hud, patched, header)
    if src.lower().endswith(".pak"):
        shell = os.path.join(work, "FLP_Shell.bin")
        run(os.path.join(here, "extract_hud.py"), src, shell, "FLP_Shell")
        shell_patched = os.path.join(work, "FLP_Shell.patched.bin")
        run(os.path.join(here, "flp_patch_shell.py"), shell, shell_patched)
        run(os.path.join(here, "make_delta.py"), shell, shell_patched, header_shell, "shell")

main()
