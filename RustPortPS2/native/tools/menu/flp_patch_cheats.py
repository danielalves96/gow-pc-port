"""Builds a patched FLP_HUD: a "Port options" entry in the pause menu that opens a list with "Cheats", "In Game Overlay" and
"Image Adjustments".
Usage: flp_patch_cheats.py <FLP_HUD.bin in> <patched out>   (also writes <out>.t9 and <out>.split)
Everything is derived from the movie it receives; any unexpected shape raises (the caller must then not patch).

Design:
  * Pause menu (top level): one more row, "Port Options", between Options and Restart (PauseMenu_State 6; the states of the other
    rows keep their numbers). Restart and Exit move one row down (the "Olympus" variant of the menu is squeezed to six rows).
  * Second level = a new page of the PauseMenu clip (label "PortOptions", frames 26 and 27): a list with "Cheats", "In Game
    Overlay", "Image Adjustments" and "Back". Its sub-state is PausePort_State: 0 = on the pause menu row, 1 = Cheats, 2 = In Game
    Overlay, 3 = Image Adjustments, 4 = Back, 5 / 6 / 7 = the Cheats / In Game Overlay / Image Adjustments page is open.
  * Third level, third page = the Image Adjustments page (label "Image", frames 30 and 31): four sliders (brightness, contrast,
    gamma, saturation: clones of the Music Volume row of the Options page, whose bar has 11 positions), "Reset to Default" and
    Back, with the sub-state PauseImage_State 1..6 (+1 for Back). The variable of a slider is the Shade Boost percentage itself
    (0..100, 50 = no change, IMAGE_STEP per press); the bar shows value / 10.
  * Third level = the Cheats page (label "Cheats", frames 24 and 25): one ON/OFF row per cheat of the F2 menu and Back, with the
    sub-state PauseCheats_State 1..N (+1 for Back). Back and Triangle return to the list.
  * Third level, second page = the In Game Overlay page (label "Overlay", frames 28 and 29): one ON/OFF row per part of the
    performance OSD (FPS counter, GPU, CPU, RAM) and Back, with the sub-state PauseOverlay_State 1..M (+1 for Back). Coming back
    from it, the list highlights In Game Overlay (PausePort_Return = 2), not the first row.
  * Handlers patched (AVM1): PressUp/PressDown/PressRight/PressLeft (root frames 9..12), X release (T5[3] entry 0) and Triangle
    release (T5[4] entry 0) get a block for PauseMenu_State == 6; PauseReset clears the sub-states. <Name>Call (copies of
    WideScreenCall) toggles PS2_<Name> for the cheats; the overlay rows carry the same toggle inline, because the free root frame
    numbers (49..58) cannot hold four more subroutines. The Options page is left exactly as the game has it.
"""
import copy, struct, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import flp_model as fm
import avm1asm as A
from avm1asm import Ins
from flp_helpers import Pool, copy_obj, clone, need, is_push, label_step

ROW = 630                       # twips between the rows of a page (the Options page has ten rows from -2835)
ROW0 = -2835
PAGE_TX = -340                  # twips: the right-aligned cheat rows sit about 27 px right of the screen center otherwise (measured)
# One row per cheat of the F2 menu (instance and HUD variable name, label). Variable PS2_<name>; the toggle subroutine is <name>Call.
# The order and the names are shared with gow_menu_diag.cpp (kCheatNames) and gow_cheats.cpp (kIds).
CHEATS = [("GodMode", "God Mode:"), ("OneHitKill", "One Hit Kills:"), ("InfiniteMagic", "Infinite Magic:"),
          ("InfiniteRage", "Infinite Rage:"), ("MaxOrbs", "Max Orbs:"), ("AllUpgrades", "All Upgrades:"),
          ("AllPowers", "All Powers:"), ("MaxMeters", "Max Meters:"), ("UnlockExtras", "Unlock Extras:")]
N = len(CHEATS)
# One row per part of the in-game performance OSD (gow_osd.inl). Variable PS2_<name>; shared with gow_menu_diag.cpp (kVarNames) and
# gow_overlay.h (the order: FPS, GPU, CPU, RAM). The labels are title case except the all-caps acronyms.
OVERLAY = [("OsdFps", "FPS Counter:"), ("OsdGpu", "GPU Stats:"), ("OsdCpu", "CPU Stats:"), ("OsdRam", "RAM Usage:")]
M = len(OVERLAY)
# One slider per Shade Boost setting (gs_host_present.cpp). Variable PS2_<name> holds the percentage 0..100 (50 = no change); shared with
# gow_menu_diag.cpp (kVarNames) and gow_image.h (the order: brightness, contrast, gamma, saturation).
IMAGE = [("ImgBrightness", "Brightness:"), ("ImgContrast", "Contrast:"), ("ImgGamma", "Gamma:"), ("ImgSaturation", "Saturation:")]
K = len(IMAGE)
# The Frame Rate page: the launcher's FPS limit (a three-way row, 60 / 120 / 240), VSync and Early FINISH. Variable PS2_<name>: FpsLimit
# holds 0, 1 or 2, the others 0 / 1; shared with gow_menu_diag.cpp (kVarNames) and gow_frame.h.
FRAMERATE = [("FpsLimit", "FPS Limit:"), ("VSync", "VSync:"), ("EarlyFinish", "Early FINISH:")]
FPS_VALUES = ["60 FPS", "120 FPS", "240 FPS"]
FPS_CHILDREN = ["Mono", "Stereo", "Surround"]     # the instance names of the three values in the Sound Mode row this row is cloned from
F = len(FRAMERATE)
# The Image Quality page: render resolution (1x..8x), upscaling filter, sharpness (a slider, SHARP_STEP percent per press), FXAA.
# Slots in the order of gow_quality.h.
QUALITY = [("Resolution", "Resolution:"), ("Upscaling", "Upscaling:"), ("Sharpness", "Sharpness:"), ("Fxaa", "FXAA:")]
Q = len(QUALITY)
RES_VALUES = ["1x", "2x", "3x", "4x", "5x", "6x", "7x", "8x"]
UP_VALUES = ["Bicubic", "Bilinear", "Pixel"]
SHARP_STEP = 5
# The rows with several values (cloned from the Sound Mode row): name -> (the texts of the values, the instance names of their clips)
SELECTS = {"FpsLimit": (FPS_VALUES, ["V0", "V1", "V2"]),
           "Resolution": (RES_VALUES, ["V%d" % k for k in range(8)]),
           "Upscaling": (UP_VALUES, ["V0", "V1", "V2"])}
# Every setting a page holds is one slot of the shared bank of HUD variables (PS2_Slot0..8, selected by PS2_PortPage; see
# gow_menu_diag.cpp): its slot is its position in the page. PAGE_ID is the PortPage value of a page.
PAGE_ID = {"cheats": 1, "overlay": 2, "image": 3, "quality": 4, "frame": 5}
SLOT = {}
for _page, _names in (("cheats", [n for n, _ in CHEATS]), ("overlay", [n for n, _ in OVERLAY]), ("image", [n for n, _ in IMAGE]),
                      ("quality", [n for n, _ in QUALITY]), ("frame", [n for n, _ in FRAMERATE])):
    for _k, _n in enumerate(_names):
        SLOT[_n] = "PS2_Slot%d" % _k
IMAGE_MAX = 100                 # the variable of a slider is the Shade Boost percentage itself, 0..IMAGE_MAX, 50 = no change
IMAGE_DEFAULT = 50
IMAGE_STEP = 2                  # percent per Left/Right press (the bar has 11 positions, so it moves every fifth press)
IMAGE_TX = -400                 # twips added to the x of the slider rows: measured at 1366 px, the block sat about 32 px right of the center with 0
PORT_STATE = 6                  # PauseMenu_State of the "Port Options" row and of everything below it
# PausePort_State values: 0 pause row, 1..6 the list rows (Cheats, In Game Overlay, Image Adjustments, Image Quality, Frame Rate, Back),
# 7..11 the open pages
PS_CHEATS, PS_OVERLAY, PS_IMAGE, PS_QUALITY, PS_FRAME, PS_BACK = 1, 2, 3, 4, 5, 6
PS_CHEATS_PAGE, PS_OVERLAY_PAGE, PS_IMAGE_PAGE, PS_QUALITY_PAGE, PS_FRAME_PAGE = 7, 8, 9, 10, 11
OVERLAY_TITLE_SCALE = 620       # font scale of the "In Game Overlay" banner: as wide as "Port Options" at 800 (5,640 vs 5,685 advance units; 1024 is the Options banner)
IMAGE_TITLE_SCALE = 540         # the same for "Image Adjustments" (5,590)
CHEATS_FRAMES = (24, 25, 26)    # page frames: init, stop, end marker (the first frame of the next page removes the layers)
PORT_FRAMES = (26, 27, 28)
OVERLAY_FRAMES = (28, 29, 30)
IMAGE_FRAMES = (30, 31, 32)
QUALITY_FRAMES = (32, 33, 34)
FRAME_FRAMES = (34, 35, 36)
# The pages that open from the list. rows: the instance names of the rows in order, the last one is Back; state: the page's
# sub-state variable; list: the list row (PausePort_State) it opens from and where the list highlights on return; page: its own
# PausePort_State.
PAGES = {"cheats": dict(rows=[n for n, _ in CHEATS] + ["CheatsBack"], state="PauseCheats_State", list=PS_CHEATS, page=PS_CHEATS_PAGE),
         "overlay": dict(rows=[n for n, _ in OVERLAY] + ["OverlayBack"], state="PauseOverlay_State", list=PS_OVERLAY, page=PS_OVERLAY_PAGE),
         "image": dict(rows=[n for n, _ in IMAGE] + ["ImageReset", "ImageBack"], state="PauseImage_State", list=PS_IMAGE, page=PS_IMAGE_PAGE),
         "quality": dict(rows=[n for n, _ in QUALITY] + ["QualityBack"], state="PauseQuality_State", list=PS_QUALITY, page=PS_QUALITY_PAGE),
         "frame": dict(rows=[n for n, _ in FRAMERATE] + ["FrameBack"], state="PauseFrame_State", list=PS_FRAME, page=PS_FRAME_PAGE)}
LIST_ROWS = [(PS_CHEATS, "PortCheats"), (PS_OVERLAY, "PortOverlay"), (PS_IMAGE, "PortImage"), (PS_QUALITY, "PortQuality"),
             (PS_FRAME, "PortFrame"), (PS_BACK, "PortBack")]
LIST_PAGE = {PS_CHEATS: ("Cheats", "cheats"), PS_OVERLAY: ("Overlay", "overlay"), PS_IMAGE: ("Image", "image"),
             PS_QUALITY: ("Quality", "quality"), PS_FRAME: ("Frame", "frame")}   # label, page
# the kind of each row of a page: toggle (ON/OFF), slider, select (several values), action (Reset), back
KINDS = {"cheats": ["toggle"] * N + ["back"], "overlay": ["toggle"] * M + ["back"],
         "image": ["slider"] * K + ["action", "back"],
         "quality": ["select", "select", "slider", "toggle", "back"],
         "frame": ["select"] + ["toggle"] * (F - 1) + ["back"]}
STEP = {"Sharpness": SHARP_STEP}   # percent per press of a slider, IMAGE_STEP for the others
TOGGLE_SRC = None               # the script of WideScreenCall (set by build()), cloned for the inline toggles
GOTO2_PAYLOAD = b"\x00"         # payload of GotoFrame2 in the movie (set by build()): a slider bar is posed with it
# top-level rows (twips): the normal menu and the "Olympus" variant that already has five rows
NORMAL_Y = {"Continue": -2000, "Options": -800, "PortOptions": 400, "Restart": 1600, "Exit": 2800}
OLYMPUS_Y = {"Continue": -2400, "Options": -1360, "PortOptions": -320, "Restart": 720, "ExitOlympus": 1760, "Exit": 2800}

def u16(b, o): return struct.unpack_from("<H", b, o)[0]

def style_len(b):
    f = b[0]
    return 1 + (4 if f & 8 else 0) + (4 if f & 4 else 0) + (2 if f & 2 else 0) + (2 if f & 1 else 0)

def text_width(blob):
    """total advance of the first glyph run of a T3 blob"""
    n = style_len(blob)
    count = blob[n]
    return sum(struct.unpack_from("<h", blob, n + 1 + 4 * k + 2)[0] for k in range(count))

def display_chars(text):
    """The HUD font draws a lower-case letter as a small capital: every word is Title case, except all-caps words of two or more
    letters ("FPS", "GPU"), which keep their capitals."""
    out = []
    for w in text.split(" "):
        core = w.rstrip(":")
        keep = len(core) >= 2 and (core.isupper() or any(c.isupper() for c in core[1:]))   # "FPS", "GPU", "VSync"
        out.append(w if keep else w[:1] + w[1:].lower())
    return " ".join(out)

def make_text(m, src, text, cmap, adv, ref=None, mode="center", centered=False, font_scale=None):
    """New T3 text in the style of T3[src]. Returns (index, applied left padding in advance units).
    centered: the labels of the pause menu are centered by the x offset of their T3 element (s16 at +0x10 = 1438 - width / 2 in
    the units of the advances, the same constant for every label of the HUD font), so the new element gets its own offset.
    ref: instead pads with space glyphs up to the width `ref` (rows whose element offset is shared with a longer label):
    mode "center" pads both sides, "right" only the left."""
    elem, blob = m.t3[src]
    need(blob[0] & 0x80, "text style record of T3[%d]" % src)
    n = style_len(blob)
    scale = struct.unpack_from("<H", blob, 3)[0] if blob[0] & 8 else 1024
    style = bytearray(blob[:n])
    if font_scale is not None:      # a smaller or larger text in the same style: the scale and the vertical offset follow it
        need(blob[0] & 8, "style with a font scale")
        if blob[0] & 1:
            yat = n - 2
            style[yat:yat + 2] = struct.pack("<h", int(round(struct.unpack_from("<h", blob, yat)[0] * font_scale / float(scale))))
        style[3:5] = struct.pack("<H", font_scale)
        scale = font_scale
    def a(g): return int(round(adv[g] * scale / 1024.0))
    glyphs = [cmap[ord(c)] for c in display_chars(text)]
    elem2 = bytearray(elem)
    if centered:
        centre = struct.unpack_from("<h", elem, 0x10)[0] + text_width(blob) / 2.0     # the same constant at every scale
        elem2[0x10:0x12] = struct.pack("<h", int(round(centre - sum(a(g) for g in glyphs) / 2.0)))
    lead = trail = 0
    if ref is not None:
        sp = a(cmap[32]); w = sum(a(g) for g in glyphs)
        if mode == "center": lead = trail = max(0, int(round((ref - w) / 2.0 / sp)))
        else: lead = max(0, int(round((ref - w) / float(sp))))
    glyphs = [cmap[32]] * lead + glyphs + [cmap[32]] * trail
    run = bytearray([len(glyphs)])
    for g in glyphs:
        run += struct.pack("<Hh", g, a(g))
    m.t3.append([elem2, bytes(style) + bytes(run)])
    return len(m.t3) - 1, lead * a(cmap[32])

def add_char(m, ty, idx):
    m.t0.append(bytearray(struct.pack("<HH", ty, idx)))
    return len(m.t0) - 1

def clone_clip(m, src, old_t3, new_t3):
    old_c = m.t0.index(bytearray(struct.pack("<HH", 4, old_t3))); new_c = add_char(m, 4, new_t3)
    clip = copy_obj(m.t6[src])
    hit = 0
    for rec, ents in clip.layers:
        for e in ents:
            if struct.unpack_from("<H", e, 2)[0] == old_c:
                e[2:4] = struct.pack("<H", new_c); hit += 1
    need(hit, "clip %d uses T3[%d]" % (src, old_t3))
    m.t6.append(clip)
    return add_char(m, 7, len(m.t6) - 1)

def clone_selectN(m, pool, row_src, label_old_t3, label_new_t3, orig_children, template, texts, names, cmap, adv, label_clip=None):
    """A clone of a row with several values (the Sound Mode row: a label and one clip per value, each with its own text) that has
    len(texts) values: the label gets `label_new_t3`, the three value clips of the original are replaced by one per entry of `texts`
    (a clip of the first value, `template` = (its clip, its T3), with the new text centered the way that one is) and the layers get the
    instance names `names`. `orig_children` are the value clips of the original row. Returns the new row's character."""
    clip = copy_obj(m.t6[row_src])
    if label_clip is None:      # the label's text is a direct child of the row clip
        swaps = {m.t0.index(bytearray(struct.pack("<HH", 4, label_old_t3))): add_char(m, 4, label_new_t3)}
    else:                       # the label sits in a clip of its own (T6[label_clip], which holds the text label_old_t3)
        swaps = {m.t0.index(bytearray(struct.pack("<HH", 7, label_clip))): clone_clip(m, label_clip, label_old_t3, label_new_t3)}
    old_value_chars = {m.t0.index(bytearray(struct.pack("<HH", 7, c))) for c in orig_children}
    first_char = m.t0.index(bytearray(struct.pack("<HH", 7, orig_children[0])))
    value_layers, others = [], []
    for rec, ents in clip.layers:
        used = {struct.unpack_from("<H", e, 2)[0] for e in ents} & old_value_chars
        (value_layers if used else others).append([rec, ents])
    need(len(value_layers) == len(orig_children), "the value layers of row %d" % row_src)
    template_layer = next(l for l in value_layers if first_char in {struct.unpack_from("<H", e, 2)[0] for e in l[1]})
    hit = 0
    for rec, ents in others:
        for e in ents:
            c = struct.unpack_from("<H", e, 2)[0]
            if c in swaps:
                e[2:4] = struct.pack("<H", swaps[c]); hit += 1
    need(hit >= 1, "the row %d uses its label" % row_src)
    used_depths = {struct.unpack_from("<H", rec, 0)[0] for rec, _ in others}
    new_layers = []
    for k, text in enumerate(texts):
        t3, _ = make_text(m, template[1], text, cmap, adv, centered=True)
        new_char = clone_clip(m, template[0], template[1], t3)
        rec = bytearray(template_layer[0])
        depth = 40 + k
        need(depth not in used_depths, "free layer depth")
        rec[0:2] = struct.pack("<H", depth)
        nm = pool.add(names[k])
        ents = [bytearray(e) for e in template_layer[1]]
        for e in ents:
            if struct.unpack_from("<H", e, 2)[0] == first_char:
                e[2:4] = struct.pack("<H", new_char)
                e[8:10] = struct.pack("<H", nm)
        new_layers.append([rec, ents])
    clip.layers = others + new_layers
    m.t6.append(clip)
    return add_char(m, 7, len(m.t6) - 1)

def entry(frame, char, a, b, name):
    return bytearray(struct.pack("<HHHHH", frame, char, a, b, name))

def label(path, lab): return label_step(path, lab)

# ---- AVM1 blocks ---------------------------------------------------------------------------------------------------
def var_is(name, value, skip):
    return [A.push_str(name), A.simple("GetVariable"), A.push_str(str(value)), A.simple("Equals"), A.simple("Not"), Ins(0x9D, b"", skip)]

def setvar(name, value): return [A.push_str(name), A.push_str(str(value)), A.simple("SetVariable")]
def sound(name): return setvar("PS2_Sound1", name)
def call(path): return [A.push_str(path), A.simple("Call")]
def goto_page(page): return [A.set_target("PauseMenu"), A.goto_label(page), A.simple("Play"), A.set_target("")]

def row_path(page, k):
    """Rows of a value page: 1..n the rows in order, the last one is Back"""
    return "/PauseMenu/" + PAGES[page]["rows"][k - 1]

def toggle_inline(name, end):
    """The body of WideScreenCall for PS2_<name>, in line (without its End), ending with a jump to `end`: flips the variable and poses
    the On/Off clips of the row. Jumps that went to the End of the subroutine go to that jump."""
    need(TOGGLE_SRC is not None and TOGGLE_SRC[-1].op == 0x00, "WideScreenCall script ends with End")
    src = clone(TOGGLE_SRC, lambda p, n=name.encode(): p.replace(b"PS2_WideScreen", SLOT[name].encode())
                .replace(b"PauseMenu/WideScreen", b"/PauseMenu/" + n).replace(b"WideScreen", n))
    last = src[-1]
    j = Ins(0x99, b"", end)
    for ins in src[:-1]:
        if ins.op in (0x99, 0x9D) and ins.target is last:
            ins.target = j
    return src[:-1] + [j]

def chain(blocks, fallthrough):
    """blocks: lists whose first six instructions are a var_is test; a failed test skips to the next block (the last one to
    `fallthrough`)."""
    for i, b in enumerate(blocks):
        b[5].target = blocks[i + 1][0] if i + 1 < len(blocks) else fallthrough
    return [x for b in blocks for x in b]

def find_state_test(items, state):
    hits = [i for i in range(len(items) - 5)
            if is_push(items[i], "PauseMenu_State") and items[i + 1].op == 0x1C and is_push(items[i + 2], str(state))
            and items[i + 3].op == 0x0E and items[i + 4].op == 0x12 and items[i + 5].op == 0x9D]
    need(len(hits) == 1, "tests of PauseMenu_State == %d: %d" % (state, len(hits)))
    return hits[0]

def insert_state_block(items, before_state, body):
    """Inserts `if PauseMenu_State == 6 { body } jump End` right before the test of `before_state`; what jumped to that test
    now reaches the new block first."""
    k = find_state_test(items, before_state)
    test, end = items[k], items[-1]
    need(end.op == 0x00, "script ends with End")
    block = var_is("PauseMenu_State", PORT_STATE, test) + body(end) + [Ins(0x99, b"", end)]
    for ins in items:
        if ins.op in (0x99, 0x9D) and ins.target is test:
            ins.target = block[0]
    items[k:k] = block
    return items

def pose_bar(name, prefix=""):
    """Poses the slider bar of row `name` from the percentage held by PS2_<name>: frame value / 10 + 1 (GotoFrame2 counts from 1; the
    game's volume bars hold the level 0..10 itself). The bar has 11 positions, so it moves once per several steps of the value; the
    division can give a fraction, which the frame jump truncates or rounds. `prefix` qualifies the variable ("_level0/:" in the page
    init, which runs inside the PauseMenu clip)."""
    return [A.set_target("/PauseMenu/%s/MusicBar" % name), A.push_str(prefix + SLOT[name]), A.simple("GetVariable"),
            A.push_str("10"), A.simple("Divide"), A.push_str("1"), A.simple("Add"), Ins(0x9F, GOTO2_PAYLOAD), A.set_target("")]

def slider_step(name, direction, end):
    """Left (-1) or Right (+1) on a slider row: the percentage moves IMAGE_STEP inside 0..100 (the same kind of test the game's volume
    rows use, plus a clamp, because a start value from the launcher need not be a multiple of the step), the sound is the one of the
    SFX volume row, the bar is posed again. Ends with a jump to `end`."""
    v = SLOT[name]
    step = STEP.get(name, IMAGE_STEP)
    pose = pose_bar(name)
    def clamp(value, upper):
        # if the variable went beyond the limit, PS2_x = limit. Less pops a (top) and b and pushes b < a: upper limit tests
        # `limit < v`, lower limit tests `v < limit`
        cond = ([A.push_str(str(value)), A.push_str(v), A.simple("GetVariable")] if upper else
                [A.push_str(v), A.simple("GetVariable"), A.push_str(str(value))])
        return cond + [A.simple("Less"), A.simple("Not"), Ins(0x9D, b"", pose[0])] + setvar(v, value)
    if direction > 0:
        test = [A.push_str(v), A.simple("GetVariable"), A.push_str(str(IMAGE_MAX)), A.simple("Less"), A.simple("Not"), Ins(0x9D, b"", pose[0])]
        op, limit = "Add", clamp(IMAGE_MAX, True)
    else:
        test = [A.push_str("0"), A.push_str(v), A.simple("GetVariable"), A.simple("Less"), A.simple("Not"), Ins(0x9D, b"", pose[0])]
        op, limit = "Subtract", clamp(0, False)
    change = [A.push_str(v), A.push_str(v), A.simple("GetVariable"), A.push_str(str(step)), A.simple(op), A.simple("SetVariable")]
    return test + change + limit + sound("SND_MM_SFX_SLIDER") + pose + [Ins(0x99, b"", end)]

def reset_sliders(end):
    """X on "Reset to Default": every slider back to its middle value (no change) and its bar posed again."""
    body = sound("SND_MM_SELECT")
    for name, _ in IMAGE:
        body += setvar(SLOT[name], IMAGE_DEFAULT) + pose_bar(name)
    return body + [Ins(0x99, b"", end)]

def select_step(name, direction, end):
    """Left / Right / X on a row with several values (FPS limit, resolution, upscaling): the next (or previous) value, cycling; the old
    value's clip goes out, the new one's comes in (the shapes of the Sound Mode row's handler)."""
    v = SLOT[name]
    texts, children = SELECTS[name]
    n = len(texts)
    J = lambda: Ins(0x99, b"", end)
    blocks = []
    for cur in range(n):
        nxt = (cur + direction) % n
        blocks.append(var_is(v, cur, None) + label("/PauseMenu/%s/%s" % (name, children[cur]), "Off") +
                      label("/PauseMenu/%s/%s" % (name, children[nxt]), "On") + setvar(v, nxt) + [J()])
    return sound("SND_MM_NAV_LR") + chain(blocks, end)

def row_action(page, k, kind, direction, end):
    """What X (`x`) or Left/Right (`lr`, `direction` -1 or +1) does on row k of a value page: a block ending with a jump to `end`, or
    None when the row does nothing for that key."""
    rows = PAGES[page]["rows"]
    row_kind = KINDS[page][k - 1]
    if page == "cheats":
        return call("/:" + rows[k - 1] + "Call") + [Ins(0x99, b"", end)]
    if row_kind == "toggle":
        return toggle_inline(rows[k - 1], end)
    if row_kind == "select":
        return select_step(rows[k - 1], direction or 1, end)
    if row_kind == "slider":
        return slider_step(rows[k - 1], direction, end) if kind == "lr" else None
    if row_kind == "action":
        return reset_sliders(end) if kind == "x" else None
    return None

def page_chain(page, kind, end, direction=0):
    """A value page that is open (PausePort_State == its page state): `kind` is up, down, lr (left/right, `direction` -1 or +1), x."""
    cfg = PAGES[page]
    st, total = cfg["state"], len(cfg["rows"])      # total counts Back, which is the last row
    J = lambda: Ins(0x99, b"", end)
    def step(k, to):
        return label(row_path(page, k), "Off") + label(row_path(page, to), "On") + setvar(st, to)
    if kind == "down":
        blocks = [var_is(st, k, None) + step(k, k + 1) + [J()] for k in range(1, total)]
        blocks.append(var_is(st, total, None) + [J()])
    elif kind == "up":
        blocks = [var_is(st, k, None) + step(k, k - 1) + [J()] for k in range(total, 1, -1)]
        blocks.append(var_is(st, 1, None) + [J()])
    else:
        blocks = []
        for k in range(1, total):
            body = row_action(page, k, kind, direction, end)
            if body is not None:
                blocks.append(var_is(st, k, None) + body)
        if kind == "x":     # Back: to the list (which highlights the row this page came from)
            blocks.append(var_is(st, total, None) + sound("SND_MM_BACK") + back_to_list(page) + [J()])
    return chain(blocks, end)

def back_to_list(page):
    """Leaves a value page for the Port Options list: clears the page's sub-state and asks the list to highlight the row it came from."""
    cfg = PAGES[page]
    return setvar(cfg["state"], 0) + setvar("PausePort_Return", cfg["list"]) + goto_page("PortOptions")

def arm(list_row, prefix=""):
    """PS2_PortPage = the page the highlighted row of the Port Options list opens (0 for Back): the bank of slots then holds its values
    when X opens it. `prefix` qualifies the variable ("_level0/:" in the init frames, which run inside the PauseMenu clip)."""
    page = LIST_PAGE.get(list_row)
    return setvar(prefix + "PS2_PortPage", PAGE_ID[page[1]] if page else 0)

def port_body(kind, direction=0):
    """The body of the PauseMenu_State == 6 block of PressUp (up), PressDown (down), PressRight/Left (lr, `direction` +1/-1), X (x),
    Triangle (tri)."""
    def body(end):
        J = lambda: Ins(0x99, b"", end)
        S = "PausePort_State"
        order = [s for s, _ in LIST_ROWS]
        path = {s: "/PauseMenu/" + n for s, n in LIST_ROWS}
        blocks = []
        if kind == "up":
            blocks = [var_is(S, 0, None) + label("PauseMenu/PortOptions", "Off") + label("PauseMenu/Options", "On") +
                      setvar("PauseMenu_State", 2) + [J()],
                      var_is(S, order[0], None) + [J()]]
            blocks += [var_is(S, s, None) + label(path[s], "Off") + label(path[p], "On") + setvar(S, p) + arm(p) + [J()]
                       for p, s in zip(order, order[1:])]
        elif kind == "down":
            blocks = [var_is(S, 0, None) + label("PauseMenu/PortOptions", "Off") + label("PauseMenu/Restart", "On") +
                      setvar("PauseMenu_State", 3) + [J()]]
            blocks += [var_is(S, s, None) + label(path[s], "Off") + label(path[n], "On") + setvar(S, n) + arm(n) + [J()]
                       for s, n in zip(order, order[1:])]
            blocks.append(var_is(S, order[-1], None) + [J()])
        elif kind == "x":
            blocks = [var_is(S, 0, None) + sound("SND_MM_SELECT") + goto_page("PortOptions") + [J()]]
            blocks += [var_is(S, s, None) + sound("SND_MM_SELECT") + goto_page(lab) + [J()] for s, (lab, _) in LIST_PAGE.items()]
            blocks.append(var_is(S, PS_BACK, None) + setvar(S, 0) + call("/:OpenPause") + [J()])
        elif kind == "tri":
            blocks = [var_is(S, 0, None) + setvar("PS2_Menu_Choice", 0) + sound("SND_MM_CONTINUE") + call("/:ClosePause") + [J()]]
            blocks += [var_is(S, s, None) + setvar(S, 0) + call("/:OpenPause") + [J()] for s in order]
            blocks += [var_is(S, cfg["page"], None) + sound("SND_MM_BACK") + back_to_list(page) + [J()] for page, cfg in PAGES.items()]
        if kind != "tri":
            for page, cfg in PAGES.items():
                blocks.append(var_is(S, cfg["page"], None) + page_chain(page, "lr" if kind == "lr" else kind, end, direction))
        return chain(blocks, end)
    return body

def retarget_neighbor(items, after_test, old_target, new_target, old_state, new_state):
    """The block that follows the test of PauseMenu_State == `after_test` moves its highlight from `old_target` to `new_target`
    and sets PauseMenu_State = `new_state` instead of `old_state`; PausePort_State is cleared with it (a stale value from an earlier
    visit of the Port options pages must not survive)."""
    t = find_state_test(items, after_test)
    k = next(i for i in range(t, len(items)) if items[i].op == 0x8B and items[i].payload == old_target.encode() + b"\0")
    need(items[k + 1].op == 0x8C and items[k + 1].payload == b"On\0" and items[k + 2].op == 0x06 and items[k + 3].op == 0x8B,
         "highlight step of %s" % old_target)
    need(is_push(items[k + 4], "PauseMenu_State") and is_push(items[k + 5], str(old_state)) and items[k + 6].op == 0x1D,
         "state set of %s" % old_target)
    items[k] = A.set_target(new_target)
    items[k + 5] = A.push_str(str(new_state))
    items[k + 7:k + 7] = setvar("PausePort_State", 0)

def patch_updown(items, down):
    """Down: Options -> Port Options -> Restart. Up: Restart -> Port Options -> Options."""
    if down:
        # the step Options -> Restart (state 2 with PauseOptions_State == 0) now stops at Port Options (state 6)
        retarget_neighbor(items, 2, "PauseMenu/Restart", "PauseMenu/PortOptions", 3, PORT_STATE)
        return insert_state_block(items, 3, port_body("down"))
    # the step Restart -> Options (state 3) now stops at Port Options
    retarget_neighbor(items, 3, "PauseMenu/Options", "PauseMenu/PortOptions", 2, PORT_STATE)
    return insert_state_block(items, 3, port_body("up"))

def patch_leftright(items, direction):
    return insert_state_block(items, 2, port_body("lr", direction))

def patch_x(items):
    return insert_state_block(items, 3, port_body("x"))

def patch_triangle(items):
    return insert_state_block(items, 3, port_body("tri"))

def patch_reset(items):
    """PauseReset also clears the sub-states of the Port options pages."""
    need(items[-1].op == 0x00, "PauseReset ends with End")
    items[-1:-1] = (setvar("PausePort_State", 0) + setvar("PauseCheats_State", 0) + setvar("PauseOverlay_State", 0) +
                    setvar("PauseImage_State", 0) + setvar("PauseQuality_State", 0) + setvar("PauseFrame_State", 0) + setvar("PausePort_Return", 0))
    return items

def build(d):
    global TOGGLE_SRC, GOTO2_PAYLOAD
    m = fm.parse(d)
    need(len(m.t0) == 936 and len(m.t6) == 381 and len(m.t3) == 199 and len(m.t2) == 1 and len(m.t7.frames) == 52
         and len(m.t5) == 11, "counts")
    pool = Pool(m.t10)
    t2 = m.t2[0]
    adv = struct.unpack("<%dH" % fm.u32(t2.elem, 0), t2.u16s)
    cmap = struct.unpack("<256H", t2.blk)
    pause = m.t6[233]
    # ---- find the rows of the Options page (layer entries at frame 7): their layers and T8 are templates for the new pages ----
    rows = {}
    for rec, ents in pause.layers:
        for e in ents:
            f, c, a, b, n = struct.unpack("<HHHHH", e)
            nm = pool.name(n)
            if f == 7 and c and nm in ("ProgressiveScan", "Controller", "Vibration", "Return", "TextOptions", "ButtonBack", "MusicVolume", "SoundMode"):
                rows[nm] = (rec, ents, a, b, n, c)
    need(len(rows) == 8, "Options rows found: %s" % sorted(rows))
    def t8_of(name): return rows[name][2]
    # ---- the top-level rows of the pause menu ---------------------------------------------------------------------------
    top = {}
    for rec, ents in pause.layers:
        for e in ents:
            if u16(e, 2) and pool.name(u16(e, 8)) in ("Continue", "Options", "Restart", "Exit", "ExitOlympus"):
                top[pool.name(u16(e, 8))] = (rec, ents)
    need(len(top) == 5, "pause rows found: %s" % sorted(top))
    def top_y(name, frame):
        return (NORMAL_Y if frame == 5 else OLYMPUS_Y).get(name)
    for name, (rec, ents) in top.items():
        for e in ents:
            f = u16(e, 0)
            if f in (5, 6) and u16(e, 2) and top_y(name, f) is not None:
                t = bytearray(m.t8[u16(e, 4)]); t[0x12:0x14] = struct.pack("<h", top_y(name, f))
                m.t8.append(t); e[4:6] = struct.pack("<H", len(m.t8) - 1)      # a copy: some of these T8 entries are shared
    # ---- new texts and clips ----------------------------------------------------------------------------------------------
    t3_port, _ = make_text(m, 57, "Port Options", cmap, adv, centered=True)          # style of the "Options" row
    ch_port = clone_clip(m, 189, 57, t3_port)                                          # a clone of the Options row clip
    t3_ptitle, _ = make_text(m, 75, "Port Options", cmap, adv, centered=True, font_scale=800)   # the OPTIONS banner style, smaller to fit
    ch_ptitle = clone_clip(m, 216, 75, t3_ptitle)
    t3_pcheats, _ = make_text(m, 62, "Cheats", cmap, adv, centered=True)             # style of the Controller row
    ch_pcheats = clone_clip(m, 196, 62, t3_pcheats)
    t3_pback, _ = make_text(m, 61, "Back", cmap, adv, centered=True)                 # style of the Return row
    ch_pback = clone_clip(m, 195, 61, t3_pback)
    t3_ctitle, _ = make_text(m, 75, "Cheats", cmap, adv, centered=True)
    ch_ctitle = clone_clip(m, 216, 75, t3_ctitle)
    t3_cback, _ = make_text(m, 61, "Back", cmap, adv, centered=True)
    ch_cback = clone_clip(m, 195, 61, t3_cback)
    # one value row per cheat, cloned from "Progressive Scan:" (the widest label of the Options page): every label ends where that
    # one ends, so the names line up on the right and the values (Off/On, arrows) keep their place in a column
    ref_prog = text_width(m.t3[72][1])
    ch_rows = []
    for name, text in CHEATS:
        t3, _ = make_text(m, 72, text, cmap, adv, ref=ref_prog, mode="right")
        ch_rows.append(clone_clip(m, 212, 72, t3))
    # the In Game Overlay entry of the list, its page banner (a smaller font keeps it inside the plate), its Back and its value rows
    t3_poverlay, _ = make_text(m, 62, "In Game Overlay", cmap, adv, centered=True)     # style of the Controller row
    ch_poverlay = clone_clip(m, 196, 62, t3_poverlay)
    t3_otitle, _ = make_text(m, 75, "In Game Overlay", cmap, adv, centered=True, font_scale=OVERLAY_TITLE_SCALE)
    ch_otitle = clone_clip(m, 216, 75, t3_otitle)
    t3_oback, _ = make_text(m, 61, "Back", cmap, adv, centered=True)
    ch_oback = clone_clip(m, 195, 61, t3_oback)
    ch_orows = []
    for name, text in OVERLAY:
        t3, _ = make_text(m, 72, text, cmap, adv, ref=ref_prog, mode="right")
        ch_orows.append(clone_clip(m, 212, 72, t3))
    # the Image Adjustments entry of the list, its banner, Reset to Default, Back, and the slider rows: clones of the Music Volume row
    # (T6[208]: label T3[68] plus the 11-step bar), their names ending where "Music Volume:" ends
    t3_pimage, _ = make_text(m, 62, "Image Adjustments", cmap, adv, centered=True)
    ch_pimage = clone_clip(m, 196, 62, t3_pimage)
    t3_ititle, _ = make_text(m, 75, "Image Adjustments", cmap, adv, centered=True, font_scale=IMAGE_TITLE_SCALE)
    ch_ititle = clone_clip(m, 216, 75, t3_ititle)
    t3_ireset, _ = make_text(m, 62, "Reset to Default", cmap, adv, centered=True)
    ch_ireset = clone_clip(m, 196, 62, t3_ireset)
    t3_iback, _ = make_text(m, 61, "Back", cmap, adv, centered=True)
    ch_iback = clone_clip(m, 195, 61, t3_iback)
    ref_music = text_width(m.t3[68][1])
    ch_irows = []
    for name, text in IMAGE:
        t3, _ = make_text(m, 68, text, cmap, adv, ref=ref_music, mode="right")
        ch_irows.append(clone_clip(m, 208, 68, t3))
    # the Frame Rate entry of the list, its banner, Back, the VSync / Early FINISH rows (toggles) and the FPS limit row (a clone of the
    # Sound Mode row: its label, and one clip per value with the texts 60 / 120 / 240 FPS in the style of Mono / Stereo / Surround)
    t3_pframe, _ = make_text(m, 62, "Frame Rate", cmap, adv, centered=True)
    ch_pframe = clone_clip(m, 196, 62, t3_pframe)
    t3_ftitle, _ = make_text(m, 75, "Frame Rate", cmap, adv, centered=True)
    ch_ftitle = clone_clip(m, 216, 75, t3_ftitle)
    t3_fback, _ = make_text(m, 61, "Back", cmap, adv, centered=True)
    ch_fback = clone_clip(m, 195, 61, t3_fback)
    ch_frows = []
    for name, text in FRAMERATE[1:]:
        t3, _ = make_text(m, 72, text, cmap, adv, ref=ref_prog, mode="right")
        ch_frows.append(clone_clip(m, 212, 72, t3))
    t3_flabel, _ = make_text(m, 63, FRAMERATE[0][1], cmap, adv, ref=text_width(m.t3[63][1]), mode="right")
    ch_fselect = clone_selectN(m, pool, 201, 63, t3_flabel, [197, 198, 199], (197, 64), FPS_VALUES, SELECTS["FpsLimit"][1], cmap, adv)
    # the Image Quality entry of the list, its banner, Back and its rows: two select rows (resolution, upscaling: clones of the Sound Mode
    # row), the Sharpness slider (a clone of the Music Volume row) and the FXAA toggle
    t3_pquality, _ = make_text(m, 62, "Image Quality", cmap, adv, centered=True)
    ch_pquality = clone_clip(m, 196, 62, t3_pquality)
    t3_qtitle, _ = make_text(m, 75, "Image Quality", cmap, adv, centered=True, font_scale=800)
    ch_qtitle = clone_clip(m, 216, 75, t3_qtitle)
    t3_qback, _ = make_text(m, 61, "Back", cmap, adv, centered=True)
    ch_qback = clone_clip(m, 195, 61, t3_qback)
    ch_qrows = []
    for name, text in QUALITY:
        if name in SELECTS:
            t3l, _ = make_text(m, 63, text, cmap, adv, ref=text_width(m.t3[63][1]), mode="right")
            ch_qrows.append(clone_selectN(m, pool, 201, 63, t3l, [197, 198, 199], (197, 64), SELECTS[name][0], SELECTS[name][1], cmap, adv))
        elif name == "Sharpness":
            t3, _ = make_text(m, 68, text, cmap, adv, ref=ref_music, mode="right")
            ch_qrows.append(clone_clip(m, 208, 68, t3))
        else:
            t3, _ = make_text(m, 72, text, cmap, adv, ref=ref_prog, mode="right")
            ch_qrows.append(clone_clip(m, 212, 72, t3))
    # ---- the "Port Options" row of the pause menu ------------------------------------------------------------------------
    nm_port = pool.add("PortOptions")
    orec, oents = top["Options"]
    depth_port = 29
    need(depth_port not in [u16(r, 0) for r, _ in pause.layers], "free layer depth 29")
    pents = [bytearray(e) for e in oents]
    for e in pents:
        if u16(e, 2):
            f = u16(e, 0)
            t = bytearray(m.t8[u16(e, 4)]); t[0x12:0x14] = struct.pack("<h", (NORMAL_Y if f == 5 else OLYMPUS_Y)["PortOptions"])
            m.t8.append(t)
            e[2:4] = struct.pack("<H", ch_port); e[4:6] = struct.pack("<H", len(m.t8) - 1); e[8:10] = struct.pack("<H", nm_port)
    prec = bytearray(orec); prec[0:2] = struct.pack("<H", depth_port)
    pause.layers.append([prec, pents])
    # ---- pages: layers -----------------------------------------------------------------------------------------------------
    nm_cback = pool.add("CheatsBack"); nm_ctitle = pool.add("TextCheats")
    nm_ptitle = pool.add("TextPortOptions"); nm_pcheats = pool.add("PortCheats"); nm_pback = pool.add("PortBack")
    nm_rows = [pool.add(name) for name, _ in CHEATS]
    nm_oback = pool.add("OverlayBack"); nm_otitle = pool.add("TextOverlay"); nm_poverlay = pool.add("PortOverlay")
    nm_orows = [pool.add(name) for name, _ in OVERLAY]
    nm_pimage = pool.add("PortImage"); nm_ititle = pool.add("TextImage"); nm_ireset = pool.add("ImageReset"); nm_iback = pool.add("ImageBack")
    nm_irows = [pool.add(name) for name, _ in IMAGE]
    nm_pframe = pool.add("PortFrame"); nm_ftitle = pool.add("TextFrame"); nm_fback = pool.add("FrameBack")
    nm_frows = [pool.add(name) for name, _ in FRAMERATE]
    nm_pquality = pool.add("PortQuality"); nm_qtitle = pool.add("TextQuality"); nm_qback = pool.add("QualityBack")
    nm_qrows = [pool.add(name) for name, _ in QUALITY]
    def new_t8(src_name, y, dx=0):
        t = bytearray(m.t8[t8_of(src_name)]); t[0x12:0x14] = struct.pack("<h", y)
        if dx: t[0x10:0x12] = struct.pack("<h", struct.unpack_from("<h", t, 0x10)[0] + dx)
        m.t8.append(t); return len(m.t8) - 1
    def page_layer(tmpl_ents, char, a, name_off, depth, frames, remove_at=None):
        out = []
        for k, e in enumerate(tmpl_ents):
            f, c, aa, bb, nn = struct.unpack("<HHHHH", e)
            if c:
                out.append(entry(frames[k], char, a if a is not None else aa, bb, name_off if name_off is not None else nn))
            else:
                out.append(entry(frames[k], 0, 0, 0, 0xFFFF))
        if remove_at is not None:
            out.append(entry(remove_at, 0, 0, 0, 0xFFFF))
        r = bytearray(8); r[0:2] = struct.pack("<H", depth); return [r, out]
    depth_base = max(u16(r, 0) for r, _ in pause.layers) + 10
    trec, tents, ta, tb, tn, tc = rows["TextOptions"]
    prog_ents = [bytearray(e) for e in rows["ProgressiveScan"][1] if u16(e, 0) in (7, 8)]
    ctl_ents = [bytearray(e) for e in rows["Controller"][1] if u16(e, 0) in (7, 8)]
    ret_ents = [e for e in rows["Return"][1] if u16(e, 0) in (7, 8)]
    mus_ents = [bytearray(e) for e in rows["MusicVolume"][1] if u16(e, 0) in (7, 8)]
    # the Cheats page (frames 24, 25)
    pause.layers.append(page_layer([bytearray(e) for e in tents if u16(e, 0) in (7, 8)], ch_ctitle, ta, nm_ctitle, depth_base, CHEATS_FRAMES, CHEATS_FRAMES[2]))
    for i in range(N):
        a = new_t8("ProgressiveScan", ROW0 + ROW * i, PAGE_TX)
        pause.layers.append(page_layer(prog_ents, ch_rows[i], a, nm_rows[i], depth_base + 10 * (i + 1), CHEATS_FRAMES, CHEATS_FRAMES[2]))
    pause.layers.append(page_layer(ret_ents, ch_cback, new_t8("Return", ROW0 + ROW * N), nm_cback, depth_base + 10 * (N + 1), CHEATS_FRAMES, CHEATS_FRAMES[2]))
    # the Port options list (frames 26, 27): Cheats and In Game Overlay on the first rows, Back on the last row like "Return to
    # Pause Menu". Its layers are removed by an entry at 28, where the In Game Overlay page begins.
    depth_port_page = depth_base + 10 * (N + 2)
    pr = PORT_FRAMES[2]
    pause.layers.append(page_layer([bytearray(e) for e in tents if u16(e, 0) in (7, 8)], ch_ptitle, ta, nm_ptitle, depth_port_page, PORT_FRAMES, pr))
    pause.layers.append(page_layer(ctl_ents, ch_pcheats, new_t8("Controller", ROW0), nm_pcheats, depth_port_page + 10, PORT_FRAMES, pr))
    pause.layers.append(page_layer(ret_ents, ch_pback, new_t8("Return", ROW0 + ROW * 9), nm_pback, depth_port_page + 20, PORT_FRAMES, pr))
    pause.layers.append(page_layer(ctl_ents, ch_poverlay, new_t8("Controller", ROW0 + ROW), nm_poverlay, depth_port_page + 30, PORT_FRAMES, pr))
    pause.layers.append(page_layer(ctl_ents, ch_pimage, new_t8("Controller", ROW0 + ROW * 2), nm_pimage, depth_port_page + 40, PORT_FRAMES, pr))
    pause.layers.append(page_layer(ctl_ents, ch_pquality, new_t8("Controller", ROW0 + ROW * 3), nm_pquality, depth_port_page + 45, PORT_FRAMES, pr))
    pause.layers.append(page_layer(ctl_ents, ch_pframe, new_t8("Controller", ROW0 + ROW * 4), nm_pframe, depth_port_page + 47, PORT_FRAMES, pr))
    # the In Game Overlay page (frames 28, 29): one ON/OFF row per part of the OSD, then Back right below the rows (like the Cheats page).
    # Its layers are removed by an entry at 30, where the Image Adjustments page begins.
    depth_ov = depth_port_page + 50
    orm = OVERLAY_FRAMES[2]
    pause.layers.append(page_layer([bytearray(e) for e in tents if u16(e, 0) in (7, 8)], ch_otitle, ta, nm_otitle, depth_ov, OVERLAY_FRAMES, orm))
    for i in range(M):
        a = new_t8("ProgressiveScan", ROW0 + ROW * i, PAGE_TX)
        pause.layers.append(page_layer(prog_ents, ch_orows[i], a, nm_orows[i], depth_ov + 10 * (i + 1), OVERLAY_FRAMES, orm))
    pause.layers.append(page_layer(ret_ents, ch_oback, new_t8("Return", ROW0 + ROW * M), nm_oback, depth_ov + 10 * (M + 1), OVERLAY_FRAMES, orm))
    # the Image Adjustments page (frames 30, 31): four sliders, a free row, Reset to Default, Back
    depth_im = depth_ov + 10 * (M + 2)
    irm = IMAGE_FRAMES[2]       # removed where the Frame Rate page begins
    pause.layers.append(page_layer([bytearray(e) for e in tents if u16(e, 0) in (7, 8)], ch_ititle, ta, nm_ititle, depth_im, IMAGE_FRAMES, irm))
    for i in range(K):
        a = new_t8("MusicVolume", ROW0 + ROW * i, IMAGE_TX)
        pause.layers.append(page_layer(mus_ents, ch_irows[i], a, nm_irows[i], depth_im + 10 * (i + 1), IMAGE_FRAMES, irm))
    pause.layers.append(page_layer(ctl_ents, ch_ireset, new_t8("Controller", ROW0 + ROW * (K + 1)), nm_ireset, depth_im + 10 * (K + 1), IMAGE_FRAMES, irm))
    pause.layers.append(page_layer(ret_ents, ch_iback, new_t8("Return", ROW0 + ROW * (K + 2)), nm_iback, depth_im + 10 * (K + 2), IMAGE_FRAMES, irm))
    # the Image Quality page (frames 32, 33): resolution and upscaling (select rows), sharpness (slider), FXAA, Back
    depth_q = depth_im + 10 * (K + 3)
    qrm = QUALITY_FRAMES[2]     # removed where the Frame Rate page begins
    snd_ents = [bytearray(e) for e in rows["SoundMode"][1] if u16(e, 0) in (7, 8)]
    pause.layers.append(page_layer([bytearray(e) for e in tents if u16(e, 0) in (7, 8)], ch_qtitle, ta, nm_qtitle, depth_q, QUALITY_FRAMES, qrm))
    for i, (name, _) in enumerate(QUALITY):
        if name in SELECTS:
            pause.layers.append(page_layer(snd_ents, ch_qrows[i], new_t8("SoundMode", ROW0 + ROW * i, PAGE_TX), nm_qrows[i], depth_q + 10 * (i + 1), QUALITY_FRAMES, qrm))
        elif name == "Sharpness":
            pause.layers.append(page_layer(mus_ents, ch_qrows[i], new_t8("MusicVolume", ROW0 + ROW * i, IMAGE_TX), nm_qrows[i], depth_q + 10 * (i + 1), QUALITY_FRAMES, qrm))
        else:
            pause.layers.append(page_layer(prog_ents, ch_qrows[i], new_t8("ProgressiveScan", ROW0 + ROW * i, PAGE_TX), nm_qrows[i], depth_q + 10 * (i + 1), QUALITY_FRAMES, qrm))
    pause.layers.append(page_layer(ret_ents, ch_qback, new_t8("Return", ROW0 + ROW * Q), nm_qback, depth_q + 10 * (Q + 1), QUALITY_FRAMES, qrm))
    # the Frame Rate page (frames 34, 35): the FPS limit row (cloned from the Sound Mode row), VSync, Early FINISH, Back
    depth_fr = depth_q + 10 * (Q + 2)
    pause.layers.append(page_layer([bytearray(e) for e in tents if u16(e, 0) in (7, 8)], ch_ftitle, ta, nm_ftitle, depth_fr, FRAME_FRAMES))
    pause.layers.append(page_layer(snd_ents, ch_fselect, new_t8("SoundMode", ROW0, PAGE_TX), nm_frows[0], depth_fr + 10, FRAME_FRAMES))
    for i in range(1, F):
        a = new_t8("ProgressiveScan", ROW0 + ROW * i, PAGE_TX)
        pause.layers.append(page_layer(prog_ents, ch_frows[i - 1], a, nm_frows[i], depth_fr + 10 * (i + 1), FRAME_FRAMES))
    pause.layers.append(page_layer(ret_ents, ch_fback, new_t8("Return", ROW0 + ROW * F), nm_fback, depth_fr + 10 * (F + 1), FRAME_FRAMES))
    # the Back-button hint (Triangle = Back) on the pages: extend the ButtonBack layer
    for rec2, ents2 in pause.layers:
        if any(u16(e, 2) and pool.name(u16(e, 8)) == "ButtonBack" and u16(e, 0) == 7 for e in ents2):
            last = max(u16(e, 0) for e in ents2); need(last < 24, "ButtonBack layer frames below 24")
            tmpl = next(e for e in ents2 if u16(e, 2) and u16(e, 0) == 7)
            n1 = bytearray(tmpl); n1[0:2] = struct.pack("<H", 24)
            ents2.append(n1)
    # the plate and the banner: the layers at depth 1 and 2 are removed at frame 23, so they are placed again for the pages
    for rec2, ents2 in pause.layers:
        d2 = u16(rec2, 0)
        if d2 in (1, 2, 7, 18, 21):
            src_frame = {1: 6, 2: 5, 7: 5, 18: 7, 21: 7}[d2]
            tmpl = next(e for e in ents2 if u16(e, 0) == src_frame and u16(e, 2))
            need(max(u16(e, 0) for e in ents2) < 24, "plate layer frames below 24")
            n1 = bytearray(tmpl); n1[0:2] = struct.pack("<H", 24); ents2.append(n1)
    # ---- pages: frame scripts ----------------------------------------------------------------------------------------------
    f5 = A.load(pause.frames[5][2][0])
    p0 = next(i for i, x in enumerate(f5) if is_push(x, "_Level0/:PS2_WideScreen"))
    q0 = next(i for i, x in enumerate(f5) if is_push(x, "_Level0/:PauseOptions_State"))
    need(f5[0].op == 0x8B and f5[0].payload.startswith(b"TextOptions") and f5[7].op == 0x8B and f5[7].payload == bytes(1),
         "title drop-shadow prologue of frame 5")
    GOTO2_PAYLOAD = next(x.payload for x in f5 if x.op == 0x9F)      # how the game poses its own volume bars
    # A value page's init: the drop-shadow of its title, its sub-states, every row posed (`poses`: one block per row), the first row
    # highlighted
    def value_page_init(title_name, first_row, port_state, state_var, poses):
        shadow = clone(f5[:8], lambda p: p.replace(b"TextOptions", title_name.encode()))
        highlight = label(first_row, "On")
        for i, pose in enumerate(poses):      # jumps that left a cloned block now land on the next step of this script
            nxt = poses[i + 1][0] if i + 1 < len(poses) else highlight[0]
            for ins in pose:
                if ins.op in (0x99, 0x9D) and ins.target is f5[q0]:
                    ins.target = nxt
        return (shadow + setvar("_level0/:PausePort_State", port_state) + setvar("_level0/:" + state_var, 1) +
                [x for pose in poses for x in pose] + highlight + label("/PauseMenu/ButtonBack", "On") + [A.simple("Play"), A.simple("End")])
    def toggle_poses(items):     # On/Off clips of a row from its variable, copied from the Options page init
        return [clone(f5[p0:q0], lambda p, n=name.encode(), v=SLOT[name].encode(): p.replace(b"PS2_WideScreen", v).replace(b"WideScreen", n))
                for name, _ in items]
    cheats_init = value_page_init("TextCheats", CHEATS[0][0], PS_CHEATS_PAGE, "PauseCheats_State", toggle_poses(CHEATS))
    overlay_init = value_page_init("TextOverlay", OVERLAY[0][0], PS_OVERLAY_PAGE, "PauseOverlay_State", toggle_poses(OVERLAY))
    image_init = value_page_init("TextImage", IMAGE[0][0], PS_IMAGE_PAGE, "PauseImage_State",
                                 [pose_bar(name, "_level0/:") for name, _ in IMAGE])
    def select_pose(name):
        """a row with several values: the clip of the current value stays (label Wait), the others are hidden (label None); jumps that
        leave the block land on f5[q0], which value_page_init redirects to the next step"""
        v = "_level0/:" + SLOT[name]
        texts, children = SELECTS[name]
        blocks = []
        for cur in range(len(texts)):
            body = []
            for k, child in enumerate(children):
                body += [A.set_target("/PauseMenu/%s/%s" % (name, child)), A.goto_label("Wait" if k == cur else "None"), A.set_target("")]
            blocks.append(var_is(v, cur, None) + body + [Ins(0x99, b"", f5[q0])])
        return chain(blocks, f5[q0])
    quality_poses = []
    for name, _ in QUALITY:
        if name in SELECTS:
            quality_poses.append(select_pose(name))
        elif name == "Sharpness":
            quality_poses.append(pose_bar(name, "_level0/:"))
        else:
            quality_poses.append(toggle_poses([(name, None)])[0])
    quality_init = value_page_init("TextQuality", QUALITY[0][0], PS_QUALITY_PAGE, "PauseQuality_State", quality_poses)
    frame_init = value_page_init("TextFrame", FRAMERATE[0][0], PS_FRAME_PAGE, "PauseFrame_State",
                                 [select_pose(FRAMERATE[0][0])] + toggle_poses(FRAMERATE[1:]))
    # Port options list: its title shadow, the sub-states, and the highlight: Cheats, or the row the player is coming back from when
    # PausePort_Return names it (the variable is consumed here)
    pshadow = clone(f5[:8], lambda p: p.replace(b"TextOptions", b"TextPortOptions"))
    tail = (setvar("_level0/:PausePort_Return", 0) + setvar("_level0/:PauseCheats_State", 0) + setvar("_level0/:PauseOverlay_State", 0) +
            setvar("_level0/:PauseImage_State", 0) + setvar("_level0/:PauseQuality_State", 0) + setvar("_level0/:PauseFrame_State", 0) +
            label("/PauseMenu/ButtonBack", "On") + [A.simple("Play"), A.simple("End")])
    pick_cheats = setvar("_level0/:PausePort_State", PS_CHEATS) + label("PortCheats", "On") + arm(PS_CHEATS, "_level0/:")
    pick_other = chain([var_is("_level0/:PausePort_Return", s, None) + setvar("_level0/:PausePort_State", s) + label(n, "On") +
                        arm(s, "_level0/:") + [Ins(0x99, b"", tail[0])] for s, n in LIST_ROWS[1:5]], pick_cheats[0])
    port_init = pshadow + pick_other + pick_cheats + tail
    need(max(u16(fr[0], 0) for fr in pause.frames) < 24 and u16(pause.hdr, 0) == 26, "pause clip frames below 24, length 26")
    for frames, name, init in ((CHEATS_FRAMES, "Cheats", cheats_init), (PORT_FRAMES, "PortOptions", port_init),
                               (OVERLAY_FRAMES, "Overlay", overlay_init), (IMAGE_FRAMES, "Image", image_init),
                               (QUALITY_FRAMES, "Quality", quality_init), (FRAME_FRAMES, "Frame", frame_init)):
        rec_init = bytearray(struct.pack("<HHIHH", frames[0], 1, 0, pool.add(name), 0)); a_init = [bytearray(8)]
        rec_stop = bytearray(struct.pack("<HHIHH", frames[1], 1, 0, 0xFFFF, 0)); a_stop = [bytearray(8)]
        pause.frames += [[rec_init, a_init, [A.assemble(init)]], [rec_stop, a_stop, [bytes([0x07, 0x00])]]]
    pause.hdr[0:2] = struct.pack("<H", FRAME_FRAMES[1] + 1)                      # the clip is now five pages longer
    # ---- handlers ---------------------------------------------------------------------------------------------------------
    root = m.t7
    wcall = next(fr for fr in root.frames if pool.name(struct.unpack_from("<H", fr[0], 8)[0]) == "WideScreenCall")
    TOGGLE_SRC = A.load(wcall[2][0])        # the overlay rows toggle with a copy of this body, in line
    for idx, nm, fn in ((9, "PressUp", lambda it: patch_updown(it, False)), (10, "PressDown", lambda it: patch_updown(it, True)),
                        (11, "PressRight", lambda it: patch_leftright(it, 1)), (12, "PressLeft", lambda it: patch_leftright(it, -1))):
        need(pool.name(u16(root.frames[idx][0], 8)) == nm, "root frame %d is %s" % (idx, nm))
        root.frames[idx][2][0] = A.assemble(fn(A.load(root.frames[idx][2][0])))
    x_entry = m.t5[3].entries[0]
    m.t5[3].entries[0] = [x_entry[0], A.assemble(patch_x(A.load(x_entry[1])))]
    t_entry = m.t5[4].entries[0]
    m.t5[4].entries[0] = [t_entry[0], A.assemble(patch_triangle(A.load(t_entry[1])))]
    reset = next(fr for fr in root.frames if pool.name(u16(fr[0], 8)) == "PauseReset")
    reset[2][0] = A.assemble(patch_reset(A.load(reset[2][0])))
    # <Name>Call: one toggle subroutine per cheat (copies of WideScreenCall), in the free root frame numbers 49 and up
    for i, (name, _) in enumerate(CHEATS):
        num = 49 + i
        rec = bytearray(wcall[0]); aent = [bytearray(a) for a in wcall[1]]
        rec[8:10] = struct.pack("<H", pool.add(name + "Call"))
        nums = [struct.unpack_from("<H", fr[0], 0)[0] for fr in root.frames]
        need(num not in nums and 48 in nums and 59 in nums and num < 59, "free frame number %d" % num)
        rec[0:2] = struct.pack("<H", num)
        blob = A.assemble(clone(A.load(wcall[2][0]), lambda p, n=name.encode(), v=SLOT[name].encode(): p.replace(b"PS2_WideScreen", v)
                                .replace(b"WideScreen", n)))
        root.frames.insert(next(j for j, x in enumerate(nums) if x > num), [rec, aent, [blob]])
    m.t10 = bytes(pool.data)
    need(len(m.t10) < 0x10000 and len(m.t8) < 0xFFFF and len(m.t0) < 0xFFFF, "table limits")
    t9 = b"".join(bytes(x) for x in m.t9)
    t8 = b"".join(bytes(x) for x in m.t8)
    # the game's heap writes into [0x1FF30, 0x20000) of the buffer the movie is parsed in (VOL_LO / VOL_HI of make_delta.py): dead padding
    return fm.serialize(m, True, True, window=(0x1FF30, 0x20000)), t9, t8

if __name__ == "__main__":
    d = open(sys.argv[1], "rb").read()
    out, t9, t8 = build(d)
    # <out> = the movie followed by the T8 table (both patched); <out>.t9 = the T9 table; <out>.split = size of the movie part
    open(sys.argv[2], "wb").write(out + t8)
    open(sys.argv[2] + ".t9", "wb").write(t9)
    open(sys.argv[2] + ".split", "w").write(str(len(out)))
    print("patched %d -> %d bytes (+ external T8 %d bytes, T9 %d bytes); fits the original buffer: %s" %
          (len(d), len(out), len(t8), len(t9), len(out) <= len(d)))
