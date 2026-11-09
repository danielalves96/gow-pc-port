"""Builds a patched FLP_Shell: a "Port Options" item in the title menu (New Game, Load, Options, Treasures, Port Options) that opens
the same list as the pause menu: "Cheats", "In Game Overlay", "Image Adjustments" and "Back".
Usage: flp_patch_shell.py <FLP_Shell.bin in> <patched out>   (also writes <out>.t9 and <out>.split)
Everything is derived from the movie it receives; any unexpected shape raises (the caller must then not patch).

Design (the title menu is a movie of its own, FLP_Shell; the variables PS2_<name> are the ones of the HUD object, so the pages work
on the same switches as the pause menu pages):
  * The title menu shows one item at a time with an up and a down arrow (ShellMenu_State 1..4). A fifth item, "Port Options" (state
    5), follows Treasures: a clone of the Options row clip, placed like the others; OpeningUp / OpeningDown know the new state and the
    arrows; the dispatcher after the main menu's exit animation (root frame 12) sends state 5 to the list page.
  * The pages are root frames 150.. (the root timeline had 150 frames): the list (150, 151), Cheats (152, 153), In Game Overlay (154,
    155), Image Adjustments (156, 157). Each page is an init frame (a label, the script that poses the rows) and a Stop frame; its
    layers have entries at both frames and the layers of a page are removed by an entry at the first frame of the next page.
  * One set of key handlers (new T5 records: X, Up, Down, Left, Right, Triangle) is placed over all the pages. They dispatch on
    ShellPort_Page (0 list, 1 Cheats, 2 Overlay, 3 Image) and ShellPort_Row (1..n, the last row of a page is Back).
  * Back from the list returns to the title menu on the Port Options item (ShellPortReturn, read by the init of root frame 2).
"""
import copy, struct, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import flp_model as fm
import avm1asm as A
from avm1asm import Ins
from flp_helpers import Pool, copy_obj, clone, need, is_push
import flp_patch_cheats as H

CHEATS, OVERLAY, IMAGE = H.CHEATS, H.OVERLAY, H.IMAGE
N, M, K = len(CHEATS), len(OVERLAY), len(IMAGE)
IMAGE_MAX, IMAGE_DEFAULT, IMAGE_STEP = H.IMAGE_MAX, H.IMAGE_DEFAULT, H.IMAGE_STEP
ROW0, ROW = 2000, 700            # first row and row pitch of the Options page of the title menu (twips, T8 +0x12)
BACK_Y = 7600                    # "Return to Main Menu" of the Options page: where Back goes on the list
OVERLAY_TITLE_SCALE, IMAGE_TITLE_SCALE = H.OVERLAY_TITLE_SCALE, H.IMAGE_TITLE_SCALE
FIRST_PAGE_FRAME = 150
FRAMERATE, QUALITY, SELECTS, SLOT, STEP, F, Q = H.FRAMERATE, H.QUALITY, H.SELECTS, H.SLOT, H.STEP, H.F, H.Q
FRAMES = {"list": (150, 151), "cheats": (152, 153), "overlay": (154, 155), "image": (156, 157), "quality": (158, 159), "frame": (160, 161)}
LABELS = {"list": "PortList", "cheats": "PortCheats", "overlay": "PortOverlay", "image": "PortImage", "quality": "PortQuality",
          "frame": "PortFrame"}
PAGE_NUM = {"list": 0, "cheats": 1, "overlay": 2, "image": 3, "quality": 4, "frame": 5}     # = H.PAGE_ID for the pages
PAGE_ORDER = ["list", "cheats", "overlay", "image", "quality", "frame"]
LIST_ROWS = ["PortCheatsRow", "PortOverlayRow", "PortImageRow", "PortQualityRow", "PortFrameRow", "PortBackRow"]
LIST_OPENS = ["cheats", "overlay", "image", "quality", "frame"]
ROWS = {"list": LIST_ROWS,
        "cheats": [n for n, _ in CHEATS] + ["CheatsBack"],
        "overlay": [n for n, _ in OVERLAY] + ["OverlayBack"],
        "image": [n for n, _ in IMAGE] + ["ImageReset", "ImageBack"],
        "quality": [n for n, _ in QUALITY] + ["QualityBack"],
        "frame": [n for n, _ in FRAMERATE] + ["FrameBack"]}
# the kind of each row of a page: toggle (ON/OFF), slider, select (the FPS limit: three values), action (opens a page / Reset), back
KINDS = dict(H.KINDS, list=["action"] * 5 + ["back"])
GOTO2_PAYLOAD = b"\x00"

def u16(b, o): return struct.unpack_from("<H", b, o)[0]

# ---- AVM1 pieces (paths are absolute from the root) -----------------------------------------------------------------------
def push(s): return A.push_str(str(s))
def setvar(name, value): return [push(name), push(value), A.simple("SetVariable")]
def sound(name): return setvar("PS2_Sound1", name)
def getvar(name): return [push(name), A.simple("GetVariable")]
def var_is(name, value, skip):
    return getvar(name) + [push(value), A.simple("Equals"), A.simple("Not"), Ins(0x9D, b"", skip)]
def label(path, lab, play=True):
    return [A.set_target(path), A.goto_label(lab)] + ([A.simple("Play")] if play else []) + [A.set_target("")]
def goto_frame(n): return [Ins(0x81, struct.pack("<H", n)), A.simple("Play")]
def goto_label_play(lab): return [A.goto_label(lab), A.simple("Play")]
def row_path(page, k): return "/" + ROWS[page][k - 1]
def chain(blocks, fallthrough):
    """blocks: lists whose first instructions are a var_is test (the If is the 6th instruction); a failed test skips to the next block"""
    for i, b in enumerate(blocks):
        b[5].target = blocks[i + 1][0] if i + 1 < len(blocks) else fallthrough
    return [x for b in blocks for x in b]
def guard(skip):
    """the three tests every key handler of the title menu starts with"""
    return (var_is("PS2_EnableButtons", 1, skip) + var_is("DiscRemoved_DisableButtons", 0, skip) + var_is("EnableButton", 1, skip))

def toggle_pose(name, prefix=""):
    """On/Off clips of a toggle row posed from PS2_<name> (the shapes of OptionOn): shown = label Wait, hidden = None"""
    shown = label("/%s/On" % name, "Wait", False) + label("/%s/Off" % name, "None", False)
    hidden = label("/%s/On" % name, "None", False) + label("/%s/Off" % name, "Wait", False)
    j = Ins(0x99, b"", None)
    t = var_is(prefix + SLOT[name], 1, hidden[0])
    out = t + shown + [j] + hidden
    j.target = None   # patched by the caller when the pose is followed by something (see poses_chain)
    return out, j

def poses_chain(poses):
    """a list of (items, end-jump) poses run one after the other"""
    flat = []
    for i, (items, j) in enumerate(poses):
        flat += items
    for i, (items, j) in enumerate(poses):
        j.target = poses[i + 1][0][0] if i + 1 < len(poses) else None
    return flat

def toggle_flip(name):
    """What Left/Right/X do on a toggle row: flips PS2_<name> and animates the On/Off clips (the shapes of OptionsRight)"""
    j = Ins(0x99, b"", None)
    turn_on = (label("/%s/Off" % name, "Off") + label("/%s/On" % name, "On") + setvar(SLOT[name], 1))
    turn_off = (label("/%s/Off" % name, "On") + label("/%s/On" % name, "Off") + setvar(SLOT[name], 0))
    body = var_is(SLOT[name], 1, turn_on[0]) + turn_off + [j] + turn_on
    return body, j

def pose_bar(name):
    """Poses the bar of slider row `name` from PS2_<name> (frame value / 10 + 1)"""
    return [A.set_target("/%s/MusicBar" % name), push(SLOT[name]), A.simple("GetVariable"), push("10"), A.simple("Divide"),
            push("1"), A.simple("Add"), Ins(0x9F, GOTO2_PAYLOAD), A.set_target("")]

def slider_step(name, direction):
    """Left (-1) / Right (+1) on a slider: the percentage moves IMAGE_STEP inside 0..100 and the bar is posed again"""
    v = SLOT[name]
    step = STEP.get(name, IMAGE_STEP)
    pose = pose_bar(name)
    def clamp(value, upper):
        cond = ([push(value)] + getvar(v)) if upper else (getvar(v) + [push(value)])
        return cond + [A.simple("Less"), A.simple("Not"), Ins(0x9D, b"", pose[0])] + setvar(v, value)
    if direction > 0:
        test = getvar(v) + [push(IMAGE_MAX), A.simple("Less"), A.simple("Not"), Ins(0x9D, b"", pose[0])]
        op, limit = "Add", clamp(IMAGE_MAX, True)
    else:
        test = [push(0)] + getvar(v) + [A.simple("Less"), A.simple("Not"), Ins(0x9D, b"", pose[0])]
        op, limit = "Subtract", clamp(0, False)
    change = [push(v)] + getvar(v) + [push(step), A.simple(op), A.simple("SetVariable")]
    return test + change + limit + sound("SND_MM_SFX_SLIDER") + pose

def reset_sliders():
    body = sound("SND_MM_SELECT")
    for name, _ in IMAGE:
        body += setvar(SLOT[name], IMAGE_DEFAULT) + pose_bar(name)
    return body

def arm(list_row):
    """PS2_PortPage = the page the highlighted row of the Port Options list opens (0 for Back): the bank of slots then holds its values
    when X opens it (gow_menu_diag.cpp)"""
    return setvar("PS2_PortPage", PAGE_NUM[LIST_OPENS[list_row - 1]] if list_row <= len(LIST_OPENS) else 0)

def open_page(page):
    return (sound("SND_MM_SELECT") + setvar("PS2_PortPage", PAGE_NUM[page]) + setvar("_level0/:ShellPort_Page", PAGE_NUM[page]) +
            goto_label_play(LABELS[page]))

def back_to_list(page):
    return sound("SND_MM_BACK") + setvar("_level0/:ShellPortListReturn", {"cheats": 1, "overlay": 2, "image": 3, "quality": 4, "frame": 5}[page]) + goto_label_play(LABELS["list"])

def back_to_title():
    return sound("SND_MM_BACK") + setvar("_level0/:ShellPort_Page", -1) + setvar("_level0/:ShellPortReturn", 1) + setvar("EnableButton", 0) + goto_frame(2)

def highlight(page, k, on):
    """Row k of a page goes to its highlighted ("On") or normal ("Off") look; the FPS limit row also shows or hides the arrows inside its
    three value clips (the shapes of OptionsDown)"""
    steps = label(row_path(page, k), "On" if on else "Off")
    if KINDS[page][k - 1] == "select":
        for child in SELECTS[ROWS[page][k - 1]][1]:
            for side in ("ArrowRight", "ArrowLeft"):
                steps += label("/%s/%s/%s" % (ROWS[page][k - 1], child, side), "On" if on else "Off")
    return steps

def select_pose(name):
    """a row with several values at page start: the clip of the current value stays (label Wait), the others are hidden (label None).
    Returns (items, final jump) like toggle_pose."""
    texts, children = SELECTS[name]
    j = Ins(0x99, b"", None)
    blocks = []
    for cur in range(len(texts)):
        body = []
        for k, child in enumerate(children):
            body += label("/%s/%s" % (name, child), "Wait" if k == cur else "None", False)
        blocks.append(var_is(SLOT[name], cur, None) + body + [Ins(0x99, b"", j)])
    items = chain(blocks, j) + [j]
    return items, j

def select_flip(name, direction, end):
    """Left / Right / X on a row with several values: the next or previous one (cycling): the old clip goes out, the new one comes in
    with its arrows (the shapes of OptionsRight)"""
    texts, children = SELECTS[name]
    n = len(texts)
    v = SLOT[name]
    blocks = []
    for cur in range(n):
        nxt = (cur + direction) % n
        old, new = "/%s/%s" % (name, children[cur]), "/%s/%s" % (name, children[nxt])
        blocks.append(var_is(v, cur, None) + label(old, "Off") + label(new, "On") + label(new + "/ArrowRight", "On") +
                      label(new + "/ArrowLeft", "On") + setvar(v, nxt))
    return sound("SND_MM_NAV_LR") + chain([b + [Ins(0x99, b"", end)] for b in blocks], end)

def row_step(page, k, to):
    return (highlight(page, k, False) + highlight(page, to, True) + setvar("_level0/:ShellPort_Row", to) + sound("SND_MM_NAV_UPDN") +
            (arm(to) if page == "list" else []))

def key_script(kind, direction=0):
    """The script of one key handler: kind up / down / lr (direction -1 / +1) / x / tri"""
    end = Ins(0x00)
    J = lambda: Ins(0x99, b"", end)
    pages = []
    for page in PAGE_ORDER:
        total = len(ROWS[page])
        blocks = []
        if kind in ("up", "down"):
            step = 1 if kind == "down" else -1
            rng = range(1, total) if step > 0 else range(total, 1, -1)
            blocks = [var_is("_level0/:ShellPort_Row", k, None) + row_step(page, k, k + step) + [J()] for k in rng]
        elif kind == "lr":
            for k in range(1, total):
                kk = KINDS[page][k - 1]
                if kk == "toggle":
                    body, j = toggle_flip(ROWS[page][k - 1]); j.target = end
                    blocks.append(var_is("_level0/:ShellPort_Row", k, None) + sound("SND_MM_NAV_LR") + body + [J()])
                elif kk == "select":
                    blocks.append(var_is("_level0/:ShellPort_Row", k, None) + select_flip(ROWS[page][k - 1], direction, end) + [J()])
                elif kk == "slider":
                    blocks.append(var_is("_level0/:ShellPort_Row", k, None) + slider_step(ROWS[page][k - 1], direction) + [J()])
        elif kind == "x":
            for k in range(1, total + 1):
                if page == "list":
                    body = open_page(LIST_OPENS[k - 1]) if k < total else back_to_title()
                    blocks.append(var_is("_level0/:ShellPort_Row", k, None) + body + [J()])
                elif k == total:
                    blocks.append(var_is("_level0/:ShellPort_Row", k, None) + back_to_list(page) + [J()])
                elif KINDS[page][k - 1] == "toggle":
                    body, j = toggle_flip(ROWS[page][k - 1]); j.target = end
                    blocks.append(var_is("_level0/:ShellPort_Row", k, None) + sound("SND_MM_SELECT") + body + [J()])
                elif KINDS[page][k - 1] == "select":
                    blocks.append(var_is("_level0/:ShellPort_Row", k, None) + select_flip(ROWS[page][k - 1], 1, end) + [J()])
                elif page == "image" and k == K + 1:
                    blocks.append(var_is("_level0/:ShellPort_Row", k, None) + reset_sliders() + [J()])
        else:   # tri
            body = back_to_title() if page == "list" else back_to_list(page)
            blocks = [var_is("_level0/:ShellPort_Row", 0, None)[:0] + body + [J()]]
        if not blocks:
            continue
        if kind == "tri":
            pages.append(var_is("_level0/:ShellPort_Page", PAGE_NUM[page], None) + blocks[0])
        else:
            pages.append(var_is("_level0/:ShellPort_Page", PAGE_NUM[page], None) + chain(blocks, end))
    g_skip = end
    items = guard(g_skip) + chain(pages, end) + [end]
    return items

def build(d):
    global GOTO2_PAYLOAD
    m = fm.parse(d)
    need(len(m.t0) == 373 and len(m.t3) == 113 and len(m.t6) == 135 and len(m.t5) == 40 and len(m.t2) == 1 and u16(m.t7.hdr, 0) == 150,
         "Shell counts")
    pool = Pool(m.t10)
    t2 = m.t2[0]
    adv = struct.unpack("<%dH" % fm.u32(t2.elem, 0), t2.u16s)
    cmap = struct.unpack("<256H", t2.blk)
    root = m.t7
    for fr in root.frames:      # how the game poses a bar
        for blob in fr[2]:
            for ins in A.load(blob):
                if ins.op == 0x9F:
                    GOTO2_PAYLOAD = ins.payload
    # ---- templates: the Options page rows (root layers with entries at frame 15), the title banner, the back hint ----------
    layer_of = {}
    for rec, ents in root.layers:
        for e in ents:
            f, c, a, b, n = struct.unpack("<HHHHH", e)
            nm = pool.name(n)
            if f == 15 and c and nm in ("ProgressiveScan", "MusicVolume", "Controller", "Return", "ButtonBack", "SoundMode"):
                layer_of[nm] = (rec, ents, a, b, c)
    for rec, ents in root.layers:      # the banner has no name: the entry at frame 15 that has the char of clip T6[42]
        for e in ents:
            f, c, a, b, n = struct.unpack("<HHHHH", e)
            if f == 15 and c and bytes(m.t0[c]) == struct.pack("<HH", 7, 42):
                layer_of["Banner"] = (rec, ents, a, b, c)
    need(len(layer_of) == 7, "Options page templates %s" % sorted(layer_of))
    def tmpl(name):
        rec, ents, a, b, c = layer_of[name]
        # the page stops at frame 20 of the Options page: its entry there is the resting state (the earlier ones are the pop-in
        # animation, with other color multipliers)
        rest = [bytearray(e) for e in ents if u16(e, 0) == 20]
        need(len(rest) == 1 and u16(rest[0], 2), "an entry at frame 20 of %s" % name)
        return [bytearray(rest[0]), bytearray(rest[0])], struct.unpack_from("<H", rest[0], 4)[0]
    # ---- the main menu item -----------------------------------------------------------------------------------------------
    t3_port, _ = H.make_text(m, 3, "Port Options", cmap, adv, centered=True)
    ch_port = H.clone_clip(m, 5, 3, t3_port)
    nm_row = pool.add("PortMain")
    src = next(ents for rec, ents in root.layers if any(u16(e, 2) and pool.name(u16(e, 8)) == "Options" and u16(e, 0) == 2 for e in ents))
    pents = [bytearray(e) for e in src if 2 <= u16(e, 0) <= 13]
    for e in pents:
        if u16(e, 2):
            e[2:4] = struct.pack("<H", ch_port); e[8:10] = struct.pack("<H", nm_row)
    depth_main = 100
    need(all(u16(r, 0) != depth_main for r, _ in root.layers), "free depth")
    r = bytearray(8); r[0:2] = struct.pack("<H", depth_main)
    root.layers.append([r, pents])
    # ---- texts and clips of the pages --------------------------------------------------------------------------------------
    ref_prog = H.text_width(m.t3[23][1])
    ref_music = H.text_width(m.t3[22][1])
    def action(text, scale=None):      # Controller Display style
        t, _ = H.make_text(m, 16, text, cmap, adv, centered=True)
        return H.clone_clip(m, 24, 16, t)
    def back(text="Back"):
        t, _ = H.make_text(m, 14, text, cmap, adv, centered=True)
        return H.clone_clip(m, 20, 14, t)
    def banner(text, scale=None):
        t, _ = H.make_text(m, 28, text, cmap, adv, centered=True, font_scale=scale)
        return H.clone_clip(m, 42, 28, t)
    def toggle(text):
        t, _ = H.make_text(m, 23, text, cmap, adv, ref=ref_prog, mode="right")
        return H.clone_clip(m, 39, 23, t)
    def slider(text):
        t, _ = H.make_text(m, 22, text, cmap, adv, ref=ref_music, mode="right")
        return H.clone_clip(m, 36, 22, t)
    def select(name, text):     # a row with several values (FPS limit, resolution, upscaling): a clone of the Sound Mode row
        t, _ = H.make_text(m, 17, text, cmap, adv, ref=H.text_width(m.t3[17][1]), mode="right")
        return H.clone_selectN(m, pool, 30, 17, t, [27, 28, 29], (27, 18), SELECTS[name][0], SELECTS[name][1], cmap, adv, label_clip=25)
    def rows_of(page_name, items):
        out = []
        for (name, text), kind in zip(items, KINDS[page_name]):
            out.append(select(name, text) if kind == "select" else slider(text) if kind == "slider" else toggle(text))
        return out
    chars = {"list": {"title": banner("Port Options", 800),
                      "rows": [action("Cheats"), action("In Game Overlay"), action("Image Adjustments"), action("Image Quality"),
                               action("Frame Rate"), back()]},
             "cheats": {"title": banner("Cheats"), "rows": [toggle(t) for _, t in CHEATS] + [back()]},
             "overlay": {"title": banner("In Game Overlay", OVERLAY_TITLE_SCALE), "rows": [toggle(t) for _, t in OVERLAY] + [back()]},
             "image": {"title": banner("Image Adjustments", IMAGE_TITLE_SCALE),
                       "rows": [slider(t) for _, t in IMAGE] + [action("Reset to Default"), back()]},
             "quality": {"title": banner("Image Quality", 800), "rows": rows_of("quality", QUALITY) + [back()]},
             "frame": {"title": banner("Frame Rate"), "rows": rows_of("frame", FRAMERATE) + [back()]}}
    kinds = KINDS
    src_layer = {"toggle": "ProgressiveScan", "slider": "MusicVolume", "action": "Controller", "back": "Return", "select": "SoundMode"}
    def new_t8(src_a, y):
        t = bytearray(m.t8[src_a]); t[0x12:0x14] = struct.pack("<h", y)
        m.t8.append(t); return len(m.t8) - 1
    def page_layer(first, char, a, name_off, depth, frames, remove_at=None):
        out = []
        for k, e in enumerate(first):
            f, c, aa, bb, nn = struct.unpack("<HHHHH", e)
            out.append(H.entry(frames[k], char, a if a is not None else aa, bb, name_off if name_off is not None else nn)
                       if c else H.entry(frames[k], 0, 0, 0, 0xFFFF))
        if remove_at is not None:
            out.append(H.entry(remove_at, 0, 0, 0, 0xFFFF))
        rec = bytearray(8); rec[0:2] = struct.pack("<H", depth); return [rec, out]
    order = PAGE_ORDER
    depth = 200
    for pi, page in enumerate(order):
        frames = FRAMES[page]
        remove_at = FRAMES[order[pi + 1]][0] if pi + 1 < len(order) else None
        t_first, t_a = tmpl("Banner")
        root.layers.append(page_layer(t_first, chars[page]["title"], t_a, pool.add("PortTitle"), depth, frames, remove_at)); depth += 10
        for i, (rowname, ch) in enumerate(zip(ROWS[page], chars[page]["rows"])):
            kind = kinds[page][i]
            first, a0 = tmpl(src_layer[kind])
            if page == "list":
                y = BACK_Y if kind == "back" else ROW0 + ROW * i
            elif kind == "back":
                y = ROW0 + ROW * (len(ROWS[page]) - 1)
            else:
                y = ROW0 + ROW * i
            root.layers.append(page_layer(first, ch, new_t8(a0, y), pool.add(rowname), depth, frames, remove_at)); depth += 10
        bfirst, ba = tmpl("ButtonBack")
        root.layers.append(page_layer(bfirst, bfirst[0] and struct.unpack_from("<H", bfirst[0], 2)[0], ba, pool.add("ButtonBack"), depth, frames, remove_at)); depth += 10
    # ---- key handlers: new T5 records placed over every page; each calls a subroutine frame (like the title menu's own handlers) -----
    base = {"x": 5, "up": 6, "down": 7, "right": 8, "left": 9, "tri": 10}
    sub_frames = {"x": "PortKeyX", "up": "PortKeyUp", "down": "PortKeyDown", "right": "PortKeyRight", "left": "PortKeyLeft", "tri": "PortKeyTri"}
    t5_char = {}
    sub_num = FRAMES["frame"][1] + 1
    for kind, idx in base.items():
        script = (key_script("lr", +1) if kind == "right" else key_script("lr", -1) if kind == "left" else
                  key_script({"up": "up", "down": "down", "x": "x", "tri": "tri"}[kind]))
        rec_f = bytearray(struct.pack("<HHIHH", sub_num, 1, 0, pool.add(sub_frames[kind]), 0))
        root.frames.append([rec_f, [bytearray(8)], [A.assemble(script)]])
        sub_num += 1
        call = A.assemble([push("/:" + sub_frames[kind]), A.simple("Call"), Ins(0x00)])
        rec = copy.deepcopy(m.t5[idx])
        rec.entries = [[e, call] for e, _ in rec.entries]
        m.t5.append(rec)
        t5_char[kind] = H.add_char(m, 6, len(m.t5) - 1)
    first_frame, last_frame = FIRST_PAGE_FRAME, FRAMES["frame"][1]
    for kind, ch in t5_char.items():
        ents = [H.entry(f, ch, 886, 90, 0xFFFF) for f in range(first_frame, last_frame + 1)]
        rec = bytearray(8); rec[0:2] = struct.pack("<H", depth); depth += 10
        root.layers.append([rec, ents])
    # ---- page frames ------------------------------------------------------------------------------------------------------
    def page_init(page):
        rows = ROWS[page]
        items = setvar("EnableButton", 1) + setvar("_level0/:ShellPort_Page", PAGE_NUM[page])
        if page == "list":
            picks = []
            for s in (2, 3, 4, 5):
                picks.append(var_is("_level0/:ShellPortListReturn", s, None) + setvar("_level0/:ShellPort_Row", s) +
                             label(row_path("list", s), "On") + arm(s))
            tail = setvar("_level0/:ShellPortListReturn", 0) + label("/ButtonBack", "On") + [A.simple("Play"), A.simple("End")]
            first = setvar("_level0/:ShellPort_Row", 1) + label(row_path("list", 1), "On") + arm(1)
            for b in picks:
                b.append(Ins(0x99, b"", tail[0]))
            items += chain(picks, first[0]) + first + tail
            return items
        items += setvar("_level0/:ShellPort_Row", 1)
        poses = []
        for i, n in enumerate(rows[:-1]):
            kind = KINDS[page][i]
            if kind == "select":
                poses.append(select_pose(n))
            elif kind == "toggle":
                poses.append(toggle_pose(n))
            elif kind == "slider":
                poses.append((pose_bar(n), Ins(0x99, b"", None)))      # no jump to follow: the next pose comes right after
        flat = poses_chain([pj for pj in poses])
        after = highlight(page, 1, True) + label("/ButtonBack", "On") + [A.simple("Play"), A.simple("End")]
        if poses:
            poses[-1][1].target = after[0]
        return items + flat + after
    for page in order:
        f0, f1 = FRAMES[page]
        init = page_init(page)
        rec0 = bytearray(struct.pack("<HHIHH", f0, 1, 0, pool.add(LABELS[page]), 0))
        rec1 = bytearray(struct.pack("<HHIHH", f1, 1, 0, 0xFFFF, 0))
        root.frames.append([rec0, [bytearray(8)], [A.assemble(init)]])
        root.frames.append([rec1, [bytearray(8)], [bytes([0x07, 0x00])]])
    root.hdr[0:2] = struct.pack("<H", sub_num)
    # ---- patches of the existing scripts -------------------------------------------------------------------------------------
    frame_by_label = {}
    for fr in root.frames:
        n = u16(fr[0], 8)
        if n != 0xFFFF and pool.name(n):
            frame_by_label[pool.name(n)] = fr
    def frame_by_num(num):
        return next(fr for fr in root.frames if u16(fr[0], 0) == num)
    patch_dispatcher(frame_by_num(12))
    patch_updown(frame_by_label["OpeningUp"], frame_by_label["OpeningDown"])
    patch_title_return(frame_by_num(2))
    root.frames.sort(key=lambda fr: u16(fr[0], 0))
    m.t10 = bytes(pool.data)
    need(len(m.t10) < 0x10000 and len(m.t8) < 0xFFFF and len(m.t0) < 0xFFFF, "table limits")
    return fm.serialize(m)

def find_state_test(items, var, state):
    hits = [i for i in range(len(items) - 5)
            if is_push(items[i], var) and items[i + 1].op == 0x1C and is_push(items[i + 2], str(state)) and items[i + 3].op == 0x0E
            and items[i + 4].op == 0x12 and items[i + 5].op == 0x9D]
    need(len(hits) == 1, "tests of %s == %d: %d" % (var, state, len(hits)))
    return hits[0]

def patch_dispatcher(fr):
    """root frame 12: after the exit animation of the main menu, ShellMenu_State 5 opens the Port Options list"""
    items = A.load(fr[2][0])
    k = find_state_test(items, "_level0/:ShellMenu_State", 4)
    test = items[k + 5]
    end = items[-1]
    need(end.op == 0x00 and test.target is end, "state 4 test jumps to the end")
    block = var_is("_level0/:ShellMenu_State", 5, end) + goto_frame(FRAMES["list"][0])
    test.target = block[0]
    items[-1:-1] = block
    fr[2][0] = A.assemble(items)

def patch_updown(up, down):
    """OpeningUp / OpeningDown: state 5 (Port Options) after Treasures"""
    # ---- up: a block for state 5 that copies the one for state 4 (Treasures -> Options) with the rows moved one step
    u = A.load(up[2][0])
    k4 = find_state_test(u, "_level0/:ShellMenu_State", 4)
    end = u[-1]
    block4 = u[k4:len(u) - 1]
    b5 = clone(block4, lambda p: p)
    for ins in b5:
        if ins.op == 0x96 and ins.payload == b"\0" + b"4" + b"\0": ins.payload = b"\0" + b"5" + b"\0"
        elif ins.op == 0x96 and ins.payload == b"\0" + b"3" + b"\0": ins.payload = b"\0" + b"4" + b"\0"
        elif ins.op == 0x8B and ins.payload == b"Treasures\0": ins.payload = b"PortMain\0"
        elif ins.op == 0x8B and ins.payload == b"Options\0": ins.payload = b"Treasures\0"
    # the test of the old block now falls to the new one; the new one falls to the end
    old_false = u[k4 + 5]
    need(old_false.target is end, "state 4 test of OpeningUp jumps to the end")
    old_false.target = b5[0]
    u[len(u) - 1:len(u) - 1] = b5
    up[2][0] = A.assemble(u)
    # ---- down: the block for state 3 (Options -> Treasures) hid the down arrow; now it keeps both arrows, and a new block for
    # state 4 (Treasures -> Port Options) hides it
    dn = A.load(down[2][0])
    k3 = find_state_test(dn, "_level0/:ShellMenu_State", 3)
    k4 = find_state_test(dn, "_level0/:ShellMenu_State", 4)
    block3 = dn[k3:k4]
    new4 = clone(block3, lambda p: p)
    for ins in new4:
        if ins.op == 0x96 and ins.payload == b"\0" + b"3" + b"\0": ins.payload = b"\0" + b"4" + b"\0"
        elif ins.op == 0x96 and ins.payload == b"\0" + b"4" + b"\0": ins.payload = b"\0" + b"5" + b"\0"
        elif ins.op == 0x8B and ins.payload == b"Treasures\0": ins.payload = b"PortMain\0"
        elif ins.op == 0x8B and ins.payload == b"Options\0": ins.payload = b"Treasures\0"
    # in the old block: the down arrow stays visible and plays
    arrow = next(i for i, ins in enumerate(block3) if ins.op == 0x8B and ins.payload == b"DownArrowMain/ArrowDown\0")
    need(block3[arrow + 1].op == 0x81 and block3[arrow + 1].payload == b"\x00\x00" and block3[arrow + 2].op == 0x8B,
         "down arrow hidden in the old state 3 block")
    block3[arrow + 1].payload = b"\x01\x00"
    block3.insert(arrow + 2, A.simple("Play"))
    dn[k3:k4] = block3
    # the state 4 test now has the new block between it and the old tail (which beeps)
    t4 = find_state_test(dn, "_level0/:ShellMenu_State", 4)
    # the state 3 block's 'Jump -> end of the tests' (the end) stays; the failing test of state 3 reaches the state 4 test, which must now
    # be preceded by the copy for the new move
    s3_false = dn[find_state_test(dn, "_level0/:ShellMenu_State", 3) + 5]
    s3_false.target = dn[t4]
    # new state 4 move placed before the old state 4 test (which becomes the state 5 beep guard)
    tail_test = dn[t4]
    for ins in dn:
        if ins.op in (0x99, 0x9D) and ins.target is tail_test:
            ins.target = new4[0]
    dn[t4:t4] = new4
    # the old test of state 4 (a beep for a down press in the last item) now tests state 5
    for i, ins in enumerate(dn):
        if ins is tail_test:
            dn[i + 2].payload = b"\0" + b"5" + b"\0"
    down[2][0] = A.assemble(dn)

def patch_title_return(fr):
    """root frame 2 (the title menu appears): after coming back from the Port Options list the item is Port Options"""
    items = A.load(fr[2][0])
    need(items[-1].op == 0x00 and items[-2].op == 0x06, "frame 2 ends with Play, End")
    play = items[-2]
    return_test = var_is("_level0/:ShellPortReturn", 1, play)
    body = (setvar("_level0/:ShellPortReturn", 0) + setvar("_level0/:ShellMenu_State", 5) +
            [A.set_target("NewGame"), Ins(0x81, b"\x00\x00"), A.set_target("")] + label("PortMain", "UpOn") +
            [A.set_target("DownArrowMain/ArrowDown"), Ins(0x81, b"\x00\x00"), A.set_target("")] +
            [A.set_target("UpArrowMain/ArrowUp"), Ins(0x81, b"\x01\x00"), A.simple("Play"), A.set_target("")])
    items[-2:-2] = return_test + body
    fr[2][0] = A.assemble(items)

if __name__ == "__main__":
    d = open(sys.argv[1], "rb").read()
    out = build(d)
    open(sys.argv[2], "wb").write(out)
    print("patched %d -> %d bytes (the whole movie, parsed in a block of its own)" % (len(d), len(out)))
