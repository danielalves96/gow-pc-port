#!/usr/bin/env bash
# PC Port - installer for Linux.
#
#   ./install.sh [path/to/God of War.iso (or .chd)] [--destination DIR] [--no-optimize] [--optimize [seconds]] [--jobs N]
#                [--compiler g++|clang] [--pgo auto|on|off] [--retrain-pgo] [--vu1-memory-limit-gb N]
#                [--normal-priority] [--install-deps] [--check-deps] [--recomp-exe FILE] [--tools-exe FILE]
#
#   --destination    where game/, build/, build-gen/ and vu1_captures/ go (default: next to this script). The launcher's
#                    wizard uses it; the generated C++ still lands inside RustPortPS2/native.
#   --jobs N         build processes (default: cores minus a reserve of 2, 1 or 0, at most one per 2 GB of RAM).
#   --compiler       C++ compiler: g++ (default) or clang (clang++). Also settable via CXX=clang++ or GOW_COMPILER=clang.
#   --pgo auto|on|off  speed profile (clang only; g++ ignores it): auto (default: the profile trained on this PC if present, else
#                    the runtime-only profile shipped in RustPortPS2/native/pgo/, which holds nothing from the game, so the
#                    installation never opens the game by itself), on (also train the full profile in the optimization run
#                    and rebuild with it), or off (none). Also settable via GOW_PGO=auto|on|off.
#   --retrain-pgo    train the full speed profile again even when build/pgo/gow.profdata already exists.
#   --vu1-memory-limit-gb N  RAM budget for the VU1 AOT generator in GB (default: 4, 0 = unlimited; same as install.ps1).
#   --normal-priority  do not lower the CPU and I/O priority of the long steps (nice, ionice); by default they run lowered so the
#                    PC stays usable while the game builds.
#   --install-deps   install the missing system packages (cmake, ninja, g++/clang, git and the GL/X11/ALSA development files)
#                    with the distribution's package manager, through sudo. Without it the script asks, when it runs
#                    in a terminal, and otherwise prints the command.
#   --check-deps     only report what is missing, as "[missing] ...", "[packages] ..." and "[install-command] ..." lines
#                    (exit code 2 if something is missing). The launcher reads this.
#   --recomp-exe     a built ps2_recomp (the Rust recompiler, RustPortPS2/crates/ps2-recomp-cli). Without it the script uses
#                    bin/ps2_recomp next to itself and otherwise builds it with cargo (needs Rust and a C compiler).
#   --tools-exe      same for ps2-tools (RustPortPS2/crates/ps2-tools: ISO extraction, ELF check, VU1/IOP generators).
#
# Steps (each one is skipped if already done):
#   0. Checks the tools and libraries, the free disk space and the RAM.
#   1. Extracts the disc from the ISO into game/ (with its sector map, .disc_layout) and prepares game/IOP_MOD.
#   2. Builds the Rust tools (ps2_recomp, ps2-tools) and translates SCUS_973.99 to C++ with RustPortPS2/games/scus-97399/gow-port.toml.
#   3. Builds the game (gow-2005). The launcher is a separate download (GitHub Releases).
#      The native IOP modules are created before building from the disc's IRX files (games/scus-97399/iop.manifest).
#   4. Optimization: the VU1 microcode is created before building from the ELF (games/scus-97399/vu1.manifest, without
#      playing). If that was not possible, it runs the game for a few minutes with automatic key presses, saves the VU1 microcode and
#      the IOP modules it uses and rebuilds with them as native code (much faster). Only the first time;
#      --optimize repeats it (more areas played = more native code), --no-optimize skips it.
#
# Nothing generated from the game (disc, C++ code, microcode) leaves this computer.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
NATIVE="$ROOT/RustPortPS2/native"
PROFILE="$ROOT/RustPortPS2/games/scus-97399" # the game's recompiler configuration, function map and VU1 data
# GOW-Port: build processes: the cores minus a reserve that keeps the PC responsive (2 from 8 cores, 1 from 4, none below that),
# but at most one per 2 GB of memory (each unit of ~8 generated files with -O3 uses 1-2 GB, an estimate that has not been
# re-measured); the LTO link, one per 3 GB and at most 4. At least one. --jobs N sets it and skips the reserve.
build_jobs() {   # build_jobs CORES MEM_GB -> the number of processes
    local cores="$1" mem_gb="$2" reserve=0 by_memory
    if [ "$cores" -ge 8 ]; then reserve=2; elif [ "$cores" -ge 4 ]; then reserve=1; fi
    by_memory=$(( mem_gb / 2 > 0 ? mem_gb / 2 : 1 ))
    local jobs=$(( cores - reserve ))
    [ "$jobs" -gt "$by_memory" ] && jobs=$by_memory
    [ "$jobs" -lt 1 ] && jobs=1
    echo "$jobs"
}
MEM_GB="$(awk '/MemTotal/ {print int($2 / 1048576)}' /proc/meminfo 2>/dev/null || echo 8)"
JOBS="$(build_jobs "$(nproc 2>/dev/null || echo 4)" "$MEM_GB")"
MIN_FREE_GB=20
MIN_MEM_GB=4
RECOMMENDED_MEM_GB=8

step() { printf '\n\033[1;31m==>\033[0m \033[1m%s\033[0m\n' "$*"; }
warn() { printf '\033[1;33m[warning]\033[0m %s\n' "$*"; }
error() { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; exit 1; }

# --- System packages ----------------------------------------------------------------------------------------------
# GOW-Port: C/C++ compiler and PGO selection. Defaults to g++/gcc; pass --compiler clang (or GOW_COMPILER=clang, or CXX=clang++)
# to build with clang. Switching the default to clang later only requires changing the fallback below.
PGO="auto"
RETRAIN_PGO=0
compiler_family() {   # compiler_family NAME -> clang | gcc (anything that is not clang: g++, c++, g++-14, /usr/bin/c++...)
    case "$(basename "$1")" in *clang*) echo clang ;; *) echo gcc ;; esac
}
update_checks() {
    local pgo_tool=""
    if [ "${COMPILER_CHECK:-}" = "clang++" ] && { [ "$PGO" = "on" ] || [ "$RETRAIN_PGO" = 1 ]; }; then
        pgo_tool=" llvm-profdata"
    fi
    # GOW-Port: what the build needs, as one list the script and the launcher (--check-deps) share. The programs are looked
    # up in PATH and the libraries with pkg-config; PACKAGE_FOR maps each one to the package of the three families we know.
    CHECKS="cmake ninja $COMPILER_CHECK$pgo_tool git pkg-config gl x11 xrandr xinerama xcursor xi xkbcommon wayland-client alsa"
}
set_compiler() {   # set_compiler NAME (g++ or clang; checked strictly, it comes from --compiler or GOW_COMPILER)
    case "$1" in
        clang|clang++|*/clang++) CC_BIN="${CC:-clang}"; CXX_BIN="clang++"; COMPILER_CHECK="clang++" ;;
        gcc|g++|*/g++)           CC_BIN="${CC:-gcc}";   CXX_BIN="g++";     COMPILER_CHECK="g++" ;;
        *) error "Unknown compiler '$1' (expected g++ or clang)" ;;
    esac
    # A C compiler of the other family (CC=gcc with clang++) is replaced by the matching one.
    [ "$(compiler_family "$CC_BIN")" = "$(compiler_family "$CXX_BIN")" ] || { [ "$COMPILER_CHECK" = clang++ ] && CC_BIN=clang || CC_BIN=gcc; }
    update_checks
}
set_pgo() {   # set_pgo auto|on|off
    case "$1" in
        auto|on|off) PGO="$1" ;;
        *) error "Unknown --pgo mode '$1' (expected auto, on or off)" ;;
    esac
    update_checks
}
# CXX is only a hint here (distributions and CI often set it to c++ or g++-N): it picks the family, never fails.
if [ -n "${GOW_COMPILER:-}" ]; then set_compiler "$GOW_COMPILER"
elif [ "$(compiler_family "${CXX:-g++}")" = clang ]; then set_compiler clang
else set_compiler g++
fi
set_pgo "${GOW_PGO:-auto}"
OS_RELEASE="${GOW_OS_RELEASE:-/etc/os-release}"

distro_family() {   # arch | debian | fedora | unknown
    local id="" like=""
    if [ -r "$OS_RELEASE" ]; then
        id="$(sed -n 's/^ID=//p' "$OS_RELEASE" | head -n1 | tr -d '"')"
        like="$(sed -n 's/^ID_LIKE=//p' "$OS_RELEASE" | head -n1 | tr -d '"')"
    fi
    case " $id $like " in
        *" arch "*|*" cachyos "*|*" manjaro "*) echo arch ;;
        *" debian "*|*" ubuntu "*) echo debian ;;
        *" fedora "*|*" rhel "*) echo fedora ;;
        *) echo unknown ;;
    esac
}

package_for() {   # package_for FAMILY CHECK -> package name
    case "$2" in cmake|git) echo "$2"; return ;; esac
    case "$1:$2" in
        debian:ninja) echo ninja-build ;;           fedora:ninja) echo ninja-build ;;       arch:ninja) echo ninja ;;
        debian:g++) echo build-essential ;;         fedora:g++) echo gcc-c++ ;;             arch:g++) echo base-devel ;;
        debian:clang++) echo clang ;;               fedora:clang++) echo clang ;;           arch:clang++) echo clang ;;
        debian:llvm-profdata) echo llvm ;;          fedora:llvm-profdata) echo llvm ;;      arch:llvm-profdata) echo llvm ;;
        debian:pkg-config) echo pkg-config ;;       fedora:pkg-config) echo pkgconf-pkg-config ;; arch:pkg-config) echo pkgconf ;;
        debian:gl) echo libgl-dev ;;                fedora:gl) echo mesa-libGL-devel ;;     arch:gl) echo libglvnd ;;
        debian:x11) echo libx11-dev ;;              fedora:x11) echo libX11-devel ;;        arch:x11) echo libx11 ;;
        debian:xrandr) echo libxrandr-dev ;;        fedora:xrandr) echo libXrandr-devel ;;  arch:xrandr) echo libxrandr ;;
        debian:xinerama) echo libxinerama-dev ;;    fedora:xinerama) echo libXinerama-devel ;; arch:xinerama) echo libxinerama ;;
        debian:xcursor) echo libxcursor-dev ;;      fedora:xcursor) echo libXcursor-devel ;; arch:xcursor) echo libxcursor ;;
        debian:xi) echo libxi-dev ;;                fedora:xi) echo libXi-devel ;;          arch:xi) echo libxi ;;
        debian:xkbcommon) echo libxkbcommon-dev ;;  fedora:xkbcommon) echo libxkbcommon-devel ;; arch:xkbcommon) echo libxkbcommon ;;
        debian:wayland-client) echo libwayland-dev ;; fedora:wayland-client) echo wayland-devel ;; arch:wayland-client) echo wayland ;;
        debian:alsa) echo libasound2-dev ;;         fedora:alsa) echo alsa-lib-devel ;;     arch:alsa) echo alsa-lib ;;
    esac
}

# Fills MISSING_CHECKS and MISSING_PACKAGES (without repeats, in the order of CHECKS). Without pkg-config the libraries
# cannot be looked up, so they count as missing: installing a package that is already there does no harm.
find_missing() {   # find_missing FAMILY
    local key package have_pkg_config=1
    MISSING_CHECKS=()
    MISSING_PACKAGES=()
    command -v pkg-config >/dev/null 2>&1 || have_pkg_config=0
    for key in $CHECKS; do
        case "$key" in
            cmake|ninja|g++|clang++|llvm-profdata|git|pkg-config) ! command -v "$key" >/dev/null 2>&1 || continue ;;
            *) [ "$have_pkg_config" = 1 ] && pkg-config --exists "$key" 2>/dev/null && continue ;;
        esac
        MISSING_CHECKS+=("$key")
        package="$(package_for "$1" "$key")"
        [ -n "$package" ] || continue
        case " ${MISSING_PACKAGES[*]:-} " in *" $package "*) ;; *) MISSING_PACKAGES+=("$package") ;; esac
    done
}

install_command() {   # install_command FAMILY PACKAGE... -> the command, as it is shown to the user
    local family="$1"
    shift
    case "$family" in
        arch) echo "sudo pacman -S --needed $*" ;;
        debian) echo "sudo apt install $*" ;;
        fedora) echo "sudo dnf install $*" ;;
    esac
}

run_install() {   # run_install FAMILY PACKAGE...
    local family="$1" sudo_prefix=""
    shift
    if [ "$(id -u)" -ne 0 ]; then
        command -v sudo >/dev/null 2>&1 || error "sudo was not found: run this as root, or install the packages yourself: $(install_command "$family" "$@")"
        sudo_prefix="sudo"
    fi
    case "$family" in
        arch) $sudo_prefix pacman -S --needed --noconfirm "$@" ;;
        debian) $sudo_prefix apt-get update && $sudo_prefix apt-get install -y "$@" ;;
        fedora) $sudo_prefix dnf install -y "$@" ;;
    esac
}

report_missing() {   # report_missing FAMILY: the lines the launcher parses
    echo "[missing] ${MISSING_CHECKS[*]}"
    if [ ${#MISSING_PACKAGES[@]} -gt 0 ]; then
        echo "[packages] ${MISSING_PACKAGES[*]}"
        echo "[install-command] $(install_command "$1" "${MISSING_PACKAGES[@]}")"
    fi
}

# ensure_dependencies MODE, MODE = yes (install), ask (ask if there is a terminal) or no (just report).
ensure_dependencies() {
    local family answer=""
    family="$(distro_family)"
    find_missing "$family"
    [ ${#MISSING_CHECKS[@]} -eq 0 ] && return 0
    report_missing "$family"
    if [ "$family" = unknown ]; then
        echo "This distribution is not one of Arch, Debian/Ubuntu or Fedora (and relatives): install the equivalents of"
        echo "  cmake, ninja, a C++ compiler, git, pkg-config and the development files of GL, X11, Xrandr, Xinerama,"
        echo "  Xcursor, Xi, xkbcommon, Wayland and ALSA, then run this script again."
        exit 1
    fi
    case "$1" in
        yes) answer=y ;;
        ask) if [ -t 0 ] && [ -t 1 ]; then read -r -p "Install them now? sudo may ask for your password. [y/N] " answer || answer=n; fi ;;
    esac
    case "$answer" in
        y|Y|yes) ;;
        *) echo "Install them and run this script again (or pass --install-deps to let it do that)."; exit 1 ;;
    esac
    step "Installing the missing packages"
    run_install "$family" "${MISSING_PACKAGES[@]}"
    find_missing "$family"
    [ ${#MISSING_CHECKS[@]} -eq 0 ] || error "Still missing after the installation: ${MISSING_CHECKS[*]}"
}

# --- Disk and memory ----------------------------------------------------------------------------------------------
free_gb() {   # free_gb DIRECTORY -> whole GB available to the user, empty if it cannot be read
    { df -Pk "$1" 2>/dev/null || true; } | awk 'NR == 2 {print int($4 / 1048576)}'
}

# Errors (exit) when the machine cannot finish the installation, warns when it can but badly. GOW_SKIP_RESOURCE_CHECK=1 skips it.
check_resources() {   # check_resources DIRECTORY [already extracted: 1]
    [ -n "${GOW_SKIP_RESOURCE_CHECK:-}" ] && return 0
    local free
    free="$(free_gb "$1")"
    if [ -z "$free" ]; then
        warn "Could not read the free space of $1."
    elif [ "$free" -lt "$MIN_FREE_GB" ]; then
        if [ "${2:-0}" = 1 ]; then
            warn "Only $free GB free in $1 (about $MIN_FREE_GB GB are needed from scratch; the disc is already extracted)."
        else
            error "Only $free GB free in $1: the installation needs about $MIN_FREE_GB GB (the extracted disc, the build and the tools)."
        fi
    fi
    if [ "$MEM_GB" -lt "$MIN_MEM_GB" ]; then
        error "This machine has $MEM_GB GB of RAM: building needs at least $MIN_MEM_GB GB ($RECOMMENDED_MEM_GB GB recommended)."
    elif [ "$MEM_GB" -lt "$RECOMMENDED_MEM_GB" ]; then
        warn "This machine has $MEM_GB GB of RAM; $RECOMMENDED_MEM_GB GB are recommended. The build will use $JOBS process(es) and take longer."
    fi
}

# GOW-Port: the long steps (cargo, the recompiler, the disc extraction, the build) run at a low CPU and I/O priority so the PC stays
# usable while the game builds. The game's own optimization run does not use it. --normal-priority turns it off. ionice is used
# only if it works here (it fails in some containers), tried once.
LOW_PRIORITY=1
low_priority() {   # low_priority COMMAND [ARGS...]
    if [ "$LOW_PRIORITY" != 1 ] || ! command -v nice >/dev/null 2>&1; then "$@"; return; fi
    if [ -z "${IONICE_OK:-}" ]; then
        IONICE_OK=0
        command -v ionice >/dev/null 2>&1 && ionice -c 3 true >/dev/null 2>&1 && IONICE_OK=1
    fi
    if [ "$IONICE_OK" = 1 ]; then ionice -c 3 nice -n 10 "$@"; else nice -n 10 "$@"; fi
}

# The functions above can be tested without running an installation: GOW_INSTALL_SOURCE_ONLY=1 . install.sh
if [ -n "${GOW_INSTALL_SOURCE_ONLY:-}" ]; then return 0 2>/dev/null || exit 0; fi

DEST=""
ISO=""
RECOMP_EXE=""
TOOLS_EXE=""
OPTIMIZE="auto"
SECONDS_TO_RUN=240
VU1_MEM_LIMIT_GB=4
DEPENDENCIES="ask"
CHECK_ONLY=0

while [ $# -gt 0 ]; do
    case "$1" in
        --no-optimize) OPTIMIZE="no" ;;
        --optimize) OPTIMIZE="yes"; if [[ "${2:-}" =~ ^[0-9]+$ ]]; then SECONDS_TO_RUN="$2"; shift; fi ;;
        --jobs) JOBS="${2:?--jobs needs a number}"; shift ;;
        --compiler) set_compiler "${2:?--compiler needs g++ or clang}"; shift ;;
        --pgo) set_pgo "${2:?--pgo needs auto, on or off}"; shift ;;
        --retrain-pgo) RETRAIN_PGO=1; update_checks ;;
        --vu1-memory-limit-gb) VU1_MEM_LIMIT_GB="${2:?--vu1-memory-limit-gb needs a number}"; shift ;;
        --normal-priority) LOW_PRIORITY=0 ;;
        --destination) DEST="${2:?--destination needs a folder}"; shift ;;
        --install-deps) DEPENDENCIES="yes" ;;
        --check-deps) CHECK_ONLY=1 ;;
        --recomp-exe) RECOMP_EXE="${2:?--recomp-exe needs a file}"; shift ;;
        --tools-exe) TOOLS_EXE="${2:?--tools-exe needs a file}"; shift ;;
        -h|--help) awk 'NR > 1 && /^#/ {sub(/^# ?/, ""); print; next} NR > 1 {exit}' "$0"; exit 0 ;;
        *) ISO="$1" ;;
    esac
    shift
done

if [ "$CHECK_ONLY" = 1 ]; then
    family="$(distro_family)"
    find_missing "$family"
    [ ${#MISSING_CHECKS[@]} -eq 0 ] && exit 0
    report_missing "$family"
    exit 2
fi

DEST="${DEST:-$ROOT}"
BUILD="$DEST/build"
GAME="$DEST/game"
GEN="$DEST/build-gen"
VU1="$DEST/vu1_captures"

LTO_JOBS=$(( MEM_GB / 3 )); [ "$LTO_JOBS" -gt 4 ] && LTO_JOBS=4; [ "$LTO_JOBS" -gt "$JOBS" ] && LTO_JOBS=$JOBS
[ "$LTO_JOBS" -lt 1 ] && LTO_JOBS=1

# --- 0. Tools, disk and memory ------------------------------------------------------------------------------------
step "Checking tools"
ensure_dependencies "$DEPENDENCIES"
mkdir -p "$DEST"
EXTRACTED=0
[ -f "$GAME/SCUS_973.99" ] && [ -f "$GAME/.disc_layout" ] && EXTRACTED=1
check_resources "$DEST" "$EXTRACTED"

# GOW-Port: the Rust tools (RustPortPS2/). Order for each: its --*-exe option, then bin/<tool> (the launcher's kit), then cargo.
# Both come out of one cargo build when either is missing.
resolve_tool() {   # resolve_tool GIVEN OPTION NAME -> path, or nothing if it has to be built
    if [ -n "$1" ]; then
        [ -x "$1" ] || error "$2: $1 is not an executable file"
        echo "$1"
    elif [ -x "$ROOT/bin/$3" ]; then
        echo "$ROOT/bin/$3"
    fi
}
RECOMPILER="$(resolve_tool "$RECOMP_EXE" --recomp-exe ps2_recomp)"
TOOLS="$(resolve_tool "$TOOLS_EXE" --tools-exe ps2-tools)"
if [ -z "$RECOMPILER" ] || [ -z "$TOOLS" ]; then
    step "Building the Rust tools (ps2_recomp, ps2-tools)"
    [ -f "$ROOT/RustPortPS2/third_party/rabbitizer/Cargo.toml" ] || error 'RustPortPS2/third_party/rabbitizer is missing: the repository checkout is incomplete'
    command -v cargo >/dev/null 2>&1 || error "cargo was not found: install Rust (https://rustup.rs), or pass --recomp-exe and --tools-exe"
    low_priority cargo build --release --locked -p ps2-recomp-cli -p ps2-tools --manifest-path "$ROOT/RustPortPS2/Cargo.toml" --target-dir "$ROOT/build-rust"
    [ -n "$RECOMPILER" ] || RECOMPILER="$ROOT/build-rust/release/ps2_recomp"
    [ -n "$TOOLS" ] || TOOLS="$ROOT/build-rust/release/ps2-tools"
fi
echo "Using the recompiler: $RECOMPILER"
echo "Using ps2-tools: $TOOLS"

# --- 1. Game data -------------------------------------------------------------------------------------------------
step "Game data (game/)"
mkdir -p "$GAME"
# GOW-Port: custom textures (launcher switch): PCSX2's layout, the pack goes inside textures/replacements.
mkdir -p "$ROOT/textures/replacements"
if [ ! -f "$GAME/SCUS_973.99" ] || [ ! -f "$GAME/.disc_layout" ]; then
    if [ -z "$ISO" ]; then
        for c in "$GAME"/*.iso "$GAME"/*.ISO "$ROOT"/*.iso "$ROOT"/*.ISO "$GAME"/*.chd "$GAME"/*.CHD "$ROOT"/*.chd "$ROOT"/*.CHD; do [ -f "$c" ] && { ISO="$c"; break; }; done
    fi
    [ -n "$ISO" ] && [ -f "$ISO" ] || error "God of War ISO (USA, SCUS-97399) not found. Pass it as an argument: ./install.sh \"/path/God of War.iso\" (a .chd works too)"
    # GOW-Port (0.3.1): a CHD (chdman) is first converted to a temporary ISO in the destination (every hunk and its SHA-1
    # checked; the CHD is only read), deleted once the disc is extracted. A leftover from a failed run is replaced.
    TEMP_ISO=""
    case "$ISO" in
        *.chd|*.CHD)
            TEMP_ISO="$DEST/disc-from-chd.iso"
            rm -f "$TEMP_ISO"
            echo "Converting $ISO to a temporary ISO (about 8 GB, deleted after the extraction)..."
            low_priority "$TOOLS" chd-extract "$ISO" "$TEMP_ISO"
            ISO="$TEMP_ISO"
            ;;
    esac
    echo "Extracting $ISO (about 8 GB; the game does not need the ISO afterwards)..."
    low_priority "$TOOLS" iso-extract "$ISO" "$GAME" --layout
    [ -n "$TEMP_ISO" ] && rm -f "$TEMP_ISO"
fi
grep -q "SCUS_973.99" "$GAME/SYSTEM.CNF" 2>/dev/null || error "game/ is not God of War USA (SCUS-97399): SYSTEM.CNF does not boot SCUS_973.99."
# GOW-Port: a correct file name does not identify the executable's revision.
"$TOOLS" verify-elf "$GAME/SCUS_973.99" --map "$PROFILE/funcmap.csv"
# GOW-Port (R5): the memory card is created already formatted (like a BIOS-formatted card); an existing one is never touched.
if [ ! -e "$GAME/Mcd001.ps2" ]; then
    "$TOOLS" mc-format "$GAME/Mcd001.ps2" || echo "warning: could not create the memory card; the game will start with a blank one."
fi
mkdir -p "$GAME/IOP_MOD"
for f in "$GAME"/*.IRX; do
    n="$(basename "$f")"
    [ -f "$GAME/IOP_MOD/$n" ] || cp "$f" "$GAME/IOP_MOD/$n"
    l="$(echo "$n" | tr 'A-Z' 'a-z')"
    [ -e "$GAME/IOP_MOD/$l" ] || ln -s "$n" "$GAME/IOP_MOD/$l"
done
echo "Done: $(ls "$GAME" | wc -l) files, IOP modules in game/IOP_MOD."

# GOW-Port: the VU1 microcode is in the ELF itself. The manifest (games/scus-97399/vu1.manifest) says which piece goes at which
# micro-memory address; with it the images are created without opening the game (same as install.ps1). vu1n_heads.txt
# (games/scus-97399/vu1-heads.list) is left next to vu1_captures/, which is where CMake looks for it.
MANIFEST="$PROFILE/vu1.manifest"
if [ -f "$MANIFEST" ] && ! compgen -G "$VU1/*.bin" >/dev/null; then
    step "VU1 microcode from the ELF (without playing)"
    mkdir -p "$VU1"
    "$TOOLS" vu1-manifest apply "$GAME/SCUS_973.99" "$MANIFEST" "$VU1" \
        || warn "Could not apply the VU1 manifest to this ELF; the microcode will be captured by playing."
fi
# GOW-Port: VU1 code that the ELF declares (.DVP.ovlytab) and that no capture has seen uploaded: vu1s_*.bin images
# (ps2-tools vu1-programs), whose positions the generator takes all as possible block entries. Idempotent.
if [ -f "$MANIFEST" ] && compgen -G "$VU1/vu1_*.bin" >/dev/null; then
    "$TOOLS" vu1-programs apply "$GAME/SCUS_973.99" "$MANIFEST" "$VU1" || true
fi

# --- 2. Recompiler and game code ----------------------------------------------------------------------------------
configure() {
    # GOW-Port: a build folder configured from another source tree makes CMake stop with "does not match the source".
    # Its cache is dropped; the downloaded dependencies (_deps) are kept.
    if [ -f "$BUILD/CMakeCache.txt" ]; then
        local cached
        cached="$(sed -n 's/^CMAKE_HOME_DIRECTORY:INTERNAL=//p' "$BUILD/CMakeCache.txt" | head -n1)"
        if [ -n "$cached" ] && [ "${cached%/}" != "${NATIVE%/}" ]; then
            warn "The build folder was configured from another source ($cached): reconfiguring."
            rm -rf "$BUILD/CMakeCache.txt" "$BUILD/CMakeFiles"
        fi
    fi
    # GOW-Port: drop the cache when switching compiler family (g++ <-> clang++), keeping _deps. Only the family is
    # compared: older installs cached /usr/bin/c++ (the g++ they were built with), and treating that as another compiler
    # would rebuild all ~6,400 units. The compiler is passed only on a fresh configure, for the same reason: CMake resets
    # the whole cache when CMAKE_CXX_COMPILER changes, even from c++ to g++.
    local compiler=()
    if [ -f "$BUILD/CMakeCache.txt" ]; then
        local cached_cxx
        cached_cxx="$(sed -n 's/^CMAKE_CXX_COMPILER:[A-Z]*=//p' "$BUILD/CMakeCache.txt" | head -n1)"
        if [ -n "$cached_cxx" ] && [ "$(compiler_family "$cached_cxx")" != "$(compiler_family "$CXX_BIN")" ]; then
            warn "The build folder was configured with another compiler ($cached_cxx): reconfiguring for $CXX_BIN."
            rm -rf "$BUILD/CMakeCache.txt" "$BUILD/CMakeFiles"
        fi
    fi
    [ -f "$BUILD/CMakeCache.txt" ] || compiler=("-DCMAKE_C_COMPILER=$CC_BIN" "-DCMAKE_CXX_COMPILER=$CXX_BIN")
    local extra=()
    if [ "${PGO_MODE:-}" = "generate" ]; then
        extra+=("-DPS2X_PGO=generate")
    elif [ "${PGO_MODE:-}" = "use" ]; then
        extra+=("-DPS2X_PGO=use" "-DPS2X_PGO_PROFILE=$PGO_PROFILE")
    elif [ "${PGO_MODE:-}" = "shipped" ]; then
        extra+=("-DPS2X_PGO=shipped")
    else
        extra+=("-DPS2X_PGO=off")
    fi
    if compgen -G "$VU1/*.bin" >/dev/null; then
        extra+=("-DPS2X_VU1_MICROCODE_DIR=$VU1" "-DPS2X_VU1_GENERATOR_SOURCE=$NATIVE/tools/vu1/generate_vu1.cpp"
                "-DPS2X_TOOLS_EXE=$TOOLS" "-DPS2X_VU1_MAX_MEMORY_MB=$(( VU1_MEM_LIMIT_GB * 1024 ))")
        # GOW-Port: CMake reads vu1n_heads.txt next to the microcode folder: the profile's list plus the block entries the
        # game reached without native code while capturing (vu1n_heads_seen.txt, written by the runtime). It was never
        # written here, so on Linux the blocks entered through a register jump ran in the interpreter. Rewritten only when
        # the list changes, so an unchanged list does not rebuild the VU1 code.
        local heads_target merged
        heads_target="$(dirname "$VU1")/vu1n_heads.txt"
        merged="$({ cat "$PROFILE/vu1-heads.list" "$VU1/vu1n_heads_seen.txt" 2>/dev/null || true; } | tr -d '\r' | tr 'A-F' 'a-f' \
            | { grep -E '^[0-9a-f]+$' || true; } | sed 's/^0*\([0-9a-f]\)/\1/' | sort -u)"
        if [ -n "$merged" ] && [ "$(cat "$heads_target" 2>/dev/null || true)" != "$merged" ]; then
            printf '%s\n' "$merged" > "$heads_target"
        fi
    fi
    cmake -S "$NATIVE" -B "$BUILD" -G Ninja -DCMAKE_BUILD_TYPE=Release \
        "${compiler[@]}" \
        -DPS2X_ENABLE_RUNTIME_LOGS=OFF -DPS2X_ENABLE_AGRESSIVE_LOGS=OFF -DPS2X_ENABLE_IOP_RPC_TRACE=OFF \
        -DPS2X_BUILD_TEST=OFF -DPS2X_RUNNER_UNITY_BUILD_BATCH_SIZE=8 \
        -DPS2X_LTO_JOBS="$LTO_JOBS" \
        "${extra[@]}" >/dev/null
}

# GOW-Port: PGO. Two profiles: the full one trained on this PC from this game (the optional --pgo on; it stays in the build
# folder, like the generated code, and wins when present) and the runtime-only one shipped in the repository (built from our
# own runtime, holds nothing from the game; the default, so an installation never opens the game by itself).
PGO_DIR="$BUILD/pgo"
PGO_PROFILE="$PGO_DIR/gow.profdata"
PGO_SHIPPED="$NATIVE/pgo/runtime.profdata"
PGO_POSSIBLE=0
[ "$COMPILER_CHECK" = "clang++" ] && [ "$PGO" != "off" ] && PGO_POSSIBLE=1
HAS_PGO_PROFILE=0
[ -f "$PGO_PROFILE" ] && HAS_PGO_PROFILE=1
TRAIN_PGO=0
if [ "$PGO_POSSIBLE" = 1 ] && { { [ "$PGO" = "on" ] && [ "$HAS_PGO_PROFILE" = 0 ]; } || [ "$RETRAIN_PGO" = 1 ]; }; then
    TRAIN_PGO=1
fi
if [ "$TRAIN_PGO" = 1 ]; then
    PGO_MODE="generate"
elif [ "$PGO_POSSIBLE" = 1 ] && [ "$HAS_PGO_PROFILE" = 1 ]; then
    PGO_MODE="use"
elif [ "$PGO_POSSIBLE" = 1 ] && [ -f "$PGO_SHIPPED" ]; then
    PGO_MODE="shipped"
else
    PGO_MODE=""
fi
if [ "$COMPILER_CHECK" = "clang++" ]; then
    case "$PGO_MODE" in
        generate) echo "Speed profile (PGO): training the full profile in the optimization run" ;;
        use) echo "Speed profile (PGO): the profile trained on this PC ($PGO_PROFILE)" ;;
        shipped) echo "Speed profile (PGO): the runtime-only profile of the repository ($PGO_SHIPPED); --pgo on trains a full one" ;;
        *) echo "Speed profile (PGO): none ($([ "$PGO" = "off" ] && echo "--pgo off" || echo "no profile in the repository"))" ;;
    esac
fi

step "Configuring the build (the first time it downloads raylib, imgui and rlImGui)"
configure
step "Translating SCUS_973.99 to C++"
mkdir -p "$GEN/out"
cp "$GAME/SCUS_973.99" "$GEN/SCUS_973.99"
cp "$PROFILE/funcmap.csv" "$GEN/funcmap.csv"
sed -e "s|@ELF@|$GEN/SCUS_973.99|" -e "s|@MAP@|$GEN/funcmap.csv|" -e "s|@OUT@|$GEN/out/|" \
    "$PROFILE/gow-port.toml" > "$GEN/recomp.toml"
low_priority "$RECOMPILER" "$GEN/recomp.toml" > "$GEN/recomp.log" 2>&1 || { tail -20 "$GEN/recomp.log"; error "ps2_recomp failed (see build-gen/recomp.log)"; }
RUNNER="$NATIVE/ps2xRuntime/src/runner"
mkdir -p "$RUNNER"
copy_if_changed() { cmp -s "$1" "$2" || cp "$1" "$2"; }   # only what changed: recompilation stays incremental
for f in "$GEN"/out/*.cpp; do copy_if_changed "$f" "$RUNNER/$(basename "$f")"; done
for f in "$GEN"/out/*.h; do copy_if_changed "$f" "$NATIVE/ps2xRuntime/include/$(basename "$f")"; done
# The overrides live in ps2xRuntime/src/game now (part of the CMake target); drop the copies older versions left here.
rm -f "$RUNNER/gow_overrides.cpp" "$RUNNER/gow_pad2_packet.h" "$RUNNER/gow_gs_replay.h"
echo "$(ls "$GEN"/out/*.cpp | wc -l) C++ files in RustPortPS2/native/ps2xRuntime/src/runner."

# GOW-Port: the native IOP code needs no game session either. The text of an IRX at its load base is a function of the IRX file
# alone, so the manifest (games/scus-97399/iop.manifest: file, base, size and hash per module, no code) rebuilds the dump the game
# writes with PS2X_IOP_DUMP_MODULES from the disc's own IRX files, and iop-gen compiles it before the build.
IOP_MANIFEST="$PROFILE/iop.manifest"
if [ -f "$IOP_MANIFEST" ]; then
    step "IOP modules from the disc (without playing)"
    if "$TOOLS" iop-manifest apply "$GAME" "$IOP_MANIFEST" "$GEN/iop_disc"; then
        "$TOOLS" iop-gen "$GEN/iop_disc" "$NATIVE/ps2xIOP/src/native"
    else
        warn "Could not rebuild the IOP modules from this disc; the IOP code will be captured by the optimization run, or interpreted."
    fi
fi

# --- 3. Game ------------------------------------------------------------------------------------------
build_game() {
    local what="${1:-}"
    step "Building the game${what} (the first time takes a while: ~6,400 files and an LTO link)"
    low_priority ninja -C "$BUILD" -j"$JOBS" ps2EntryRunner
}
run_optimization() {   # run_optimization [PROFILE_DIR]
    local profile_dir="${1:-}" pgo_env=() label="is"
    if [ -n "$profile_dir" ]; then
        mkdir -p "$profile_dir"
        rm -f "$profile_dir"/*.profraw
        pgo_env=("LLVM_PROFILE_FILE=$profile_dir/gow-%p.profraw")
        label="and the speed profile are"
    fi
    step "Optimization: the game opens by itself for ${SECONDS_TO_RUN} s (do not touch the keyboard); the VU1 and IOP code ${label} saved"
    mkdir -p "$VU1" "$GEN/iop_mods"
    (cd "$NATIVE" && env "${pgo_env[@]}" GOW_VU1_CAPTURE="$VU1" PS2X_IOP_DUMP_MODULES="$GEN/iop_mods" GOW_SKIP_FMV=1 \
        GOW_PAD_TEST=1 GOW_PAD_TEST_NO_CAPTURE=1 GOW_MC0="$GEN/optimize.ps2" PS2X_GS_WORKER=1 GOW_GS_FINISH_ASYNC=1 \
        PC_CHEATS=god_mode \
        GOW_PAD_SCRIPT="5:start,12:x,20:x,28:x,40:square,41:square,42:square,44:triangle,48:lup*3,52:x,55:square,56:triangle,60:circle,64:r1,70:lleft*2,74:square,76:square,80:triangle,90:lup*4,96:x,100:square,104:l1,110:lright*2,120:square,125:triangle,140:lup*5,150:square,160:triangle,180:lup*5,200:square" \
        timeout "$SECONDS_TO_RUN" "$BUILD/ps2xRuntime/gow-2005" "$GAME/SCUS_973.99" > "$GEN/optimize.log" 2>&1) || true
    echo "VU1: $({ ls "$VU1"/*.bin 2>/dev/null || true; } | wc -l) micro-memories; IOP: $({ ls "$GEN"/iop_mods/*.bin 2>/dev/null || true; } | wc -l) modules."
    if compgen -G "$GEN/iop_mods/*.bin" >/dev/null; then
        low_priority "$TOOLS" iop-gen "$GEN/iop_mods" "$NATIVE/ps2xIOP/src/native"
    fi
}
configure
if [ "$TRAIN_PGO" = 1 ]; then
    build_game " with instrumentation for the speed profile"
else
    build_game
fi

# --- 4. Optimization (native VU1 and IOP, and the PGO training) --------------------------------------------------
if [ "$TRAIN_PGO" = 1 ]; then
    run_optimization "$PGO_DIR"
    if compgen -G "$PGO_DIR/*.profraw" >/dev/null; then
        low_priority llvm-profdata merge -o "$PGO_PROFILE" "$PGO_DIR"/*.profraw
        rm -f "$PGO_DIR"/*.profraw
        PGO_MODE="use"
    else
        warn "The optimization run wrote no speed profile (did the game close early?): the game is built with the profile of the repository instead."
        if [ -f "$PGO_SHIPPED" ]; then PGO_MODE="shipped"; else PGO_MODE=""; fi
    fi
    configure
    build_game
elif [ "$OPTIMIZE" = "yes" ] || { [ "$OPTIMIZE" = "auto" ] && ! compgen -G "$VU1/*.bin" >/dev/null; }; then
    run_optimization
    configure
    build_game
fi

step "Done"
echo "  Play with ./run_gow.sh, or with the launcher (a separate download): it detects this installation."
echo "  More native code after playing new areas: ./install.sh --optimize"
