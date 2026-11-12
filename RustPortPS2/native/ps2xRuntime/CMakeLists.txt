cmake_minimum_required(VERSION 3.20)

project(PS2Runtime VERSION 0.1.0 LANGUAGES C CXX)
if(WIN32)
    enable_language(RC) # GOW-Port: icon and version info of gow-2005.exe (resources/gow.rc)
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_C_STANDARD 99)
set(CMAKE_C_STANDARD_REQUIRED ON)

option(PS2X_ENABLE_RUNNER_UNITY_BUILD "Build ps2EntryRunner with CMake unity build" ON)
set(PS2X_RUNNER_UNITY_BUILD_BATCH_SIZE 32 CACHE STRING "Unity build batch size for ps2EntryRunner")
option(PS2X_ENABLE_RUNNER_PCH "Precompile the heavy runtime headers for ps2EntryRunner" ON)
option(PS2X_ENABLE_SCCACHE "Use sccache as compiler launcher when available" ON)

option(PS2X_ENABLE_RUNTIME_LOGS "Enable PS2 runtime logs" ON)
option(PS2X_ENABLE_AGRESSIVE_LOGS "Enable very verbose/agressive PS2 runtime logs" ON)
option(PS2X_ENABLE_IOP_RPC_TRACE "Log unhandled IOP/SIF RPC trace suggestions" ON)
option(PS2X_STRICT_RETURN_DIAGNOSTICS "Route generated JR $ra returns through runtime branch diagnostics" OFF)
option(PS2X_SHOW_WINDOWS_CONSOLE "Show a console window for ps2EntryRunner on Windows release builds" ON)
option(PS2X_ENABLE_DEBUG_UI "Build the desktop runtime debug UI" ON)

if(PS2X_ENABLE_SCCACHE)
    find_program(PS2X_SCCACHE_PROGRAM sccache)

    if(PS2X_SCCACHE_PROGRAM)
        if(CMAKE_C_COMPILER)
            set(CMAKE_C_COMPILER_LAUNCHER "${PS2X_SCCACHE_PROGRAM}")
        endif()

        set(CMAKE_CXX_COMPILER_LAUNCHER "${PS2X_SCCACHE_PROGRAM}")
        message(STATUS "Using sccache: ${PS2X_SCCACHE_PROGRAM}")
    else()
        message(STATUS "sccache not found; continuing without compiler launcher")
    endif()
endif()

include("${CMAKE_CURRENT_SOURCE_DIR}/cmake/ReleaseMode.cmake")

set(PS2X_DEFAULT_BOOT_ELF "" CACHE STRING "Guest ELF path passed directly to main() when argv is unavailable")

include(FetchContent)
set(FETCHCONTENT_QUIET FALSE)
set(BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(BUILD_GAMES OFF CACHE BOOL "" FORCE)

FetchContent_Declare(
    raylib
    GIT_REPOSITORY "https://github.com/raysan5/raylib.git"
    GIT_TAG "5.5" # we will migrate to 4.2.0 later, trust me it will be better
    # GOW-Port: only the tag, not ~500 MB of history (minutes of a first install on a slow link; idea from gow-native-pc
    # runtime patch 42). An existing checkout is kept as it is.
    GIT_SHALLOW TRUE
    GIT_PROGRESS TRUE
)
FetchContent_MakeAvailable(raylib)

if(PS2X_ENABLE_DEBUG_UI)
    FetchContent_Declare(
        imgui
        GIT_REPOSITORY https://github.com/ocornut/imgui.git
        GIT_TAG "v1.92.7-docking"
        GIT_SHALLOW TRUE
    )
    FetchContent_GetProperties(imgui)

    if(NOT imgui_POPULATED)
        FetchContent_Populate(imgui)
    endif()

    set(PS2X_IMGUI_SOURCE_DIR "${imgui_SOURCE_DIR}")

    add_library(imgui STATIC
        "${PS2X_IMGUI_SOURCE_DIR}/imgui.cpp"
        "${PS2X_IMGUI_SOURCE_DIR}/imgui_draw.cpp"
        "${PS2X_IMGUI_SOURCE_DIR}/imgui_tables.cpp"
        "${PS2X_IMGUI_SOURCE_DIR}/imgui_widgets.cpp"
        "${PS2X_IMGUI_SOURCE_DIR}/imgui_demo.cpp"
    )
    target_include_directories(imgui PUBLIC "${PS2X_IMGUI_SOURCE_DIR}")

    FetchContent_Declare(
        rlImGui
        GIT_REPOSITORY https://github.com/raylib-extras/rlImGui.git
        GIT_TAG "Raylib_5_5"
        GIT_SHALLOW TRUE
    )
    FetchContent_GetProperties(rlImGui)

    if(NOT rlimgui_POPULATED)
        FetchContent_Populate(rlImGui)
    endif()

    set(PS2X_RLIMGUI_SOURCE_DIR "${rlimgui_SOURCE_DIR}")

    add_library(rlImGui STATIC
        "${PS2X_RLIMGUI_SOURCE_DIR}/rlImGui.cpp"
    )
    target_include_directories(rlImGui PUBLIC "${PS2X_RLIMGUI_SOURCE_DIR}")
    target_link_libraries(rlImGui PUBLIC raylib imgui)
endif()

add_library(ps2_host_backend INTERFACE)

set(PS2X_DEFAULT_BOOT_ELF_DEFINE "")

if(PS2X_DEFAULT_BOOT_ELF)
    set(PS2X_DEFAULT_BOOT_ELF_DEFINE "${PS2X_DEFAULT_BOOT_ELF}")
    string(REPLACE "\\" "\\\\" PS2X_DEFAULT_BOOT_ELF_DEFINE "${PS2X_DEFAULT_BOOT_ELF_DEFINE}")
    string(REPLACE "\"" "\\\"" PS2X_DEFAULT_BOOT_ELF_DEFINE "${PS2X_DEFAULT_BOOT_ELF_DEFINE}")
endif()

target_link_libraries(ps2_host_backend INTERFACE raylib)

# GOW-Port: MPEG video (FMV) is decoded by the vendored libmpeg2 (third_party/libmpeg2, portable C, also builds for
# the Switch). FFmpeg stays available as an opt-in alternative backend.
add_library(libmpeg2 STATIC
    third_party/libmpeg2/alloc.c
    third_party/libmpeg2/cpu_accel.c
    third_party/libmpeg2/cpu_state.c
    third_party/libmpeg2/decode.c
    third_party/libmpeg2/header.c
    third_party/libmpeg2/idct.c
    third_party/libmpeg2/motion_comp.c
    third_party/libmpeg2/slice.c
)
target_include_directories(libmpeg2 PUBLIC third_party/libmpeg2)
if(MSVC)
    target_compile_options(libmpeg2 PRIVATE /W0)
endif()

# RetroAchievements: official evaluation and synchronization library. It is
# vendored so that a build does not depend on the network and so that we pin the
# behavior to the tested version (12.5.0).
set(RCHEEVOS_DIR "${CMAKE_CURRENT_SOURCE_DIR}/third_party/rcheevos")
if(NOT EXISTS "${RCHEEVOS_DIR}/include/rc_client.h")
    message(FATAL_ERROR "rcheevos 12.5.0 missing in ${RCHEEVOS_DIR}. Extract the official package before building.")
endif()

file(GLOB RCHEEVOS_RAPI_SOURCES CONFIGURE_DEPENDS "${RCHEEVOS_DIR}/src/rapi/*.c")
file(GLOB RCHEEVOS_CORE_SOURCES CONFIGURE_DEPENDS "${RCHEEVOS_DIR}/src/rcheevos/*.c")
file(GLOB RCHEEVOS_HASH_SOURCES CONFIGURE_DEPENDS "${RCHEEVOS_DIR}/src/rhash/*.c")
add_library(rcheevos STATIC
    ${RCHEEVOS_RAPI_SOURCES}
    ${RCHEEVOS_CORE_SOURCES}
    ${RCHEEVOS_HASH_SOURCES}
    "${RCHEEVOS_DIR}/src/rc_client.c"
    "${RCHEEVOS_DIR}/src/rc_compat.c"
    "${RCHEEVOS_DIR}/src/rc_util.c"
    "${RCHEEVOS_DIR}/src/rc_version.c"
)
target_include_directories(rcheevos PUBLIC "${RCHEEVOS_DIR}/include")
target_compile_definitions(rcheevos PUBLIC RC_CLIENT_SUPPORTS_HASH=1)
if(MSVC)
    target_compile_options(rcheevos PRIVATE /W0)
endif()

option(PS2X_ENABLE_FFMPEG "Decode MPEG video with FFmpeg instead of libmpeg2" OFF)

if(NOT PS2X_ENABLE_FFMPEG)
    add_library(ffmpeg INTERFACE)
elseif(WIN32)
    include(ExternalProject)

    set(FFMPEG_PREBUILT_TAG "n7.1-241205")
    set(FFMPEG_PREBUILT_URL
        "https://github.com/System233/ffmpeg-msvc-prebuilt/releases/download/${FFMPEG_PREBUILT_TAG}/ffmpeg-${FFMPEG_PREBUILT_TAG}-lgpl-amd64-shared.zip")

    set(FFMPEG_PREFIX_DIR "${CMAKE_BINARY_DIR}/ThirdParty/ffmpeg-prefix")
    set(FFMPEG_SOURCE_DIR "${FFMPEG_PREFIX_DIR}/src/ffmpeg_external")
    set(FFMPEG_INCLUDE_DIR "${FFMPEG_SOURCE_DIR}/include")
    set(FFMPEG_LIB_DIR "${FFMPEG_SOURCE_DIR}/bin")
    set(FFMPEG_BIN_DIR "${FFMPEG_SOURCE_DIR}/bin")

    ExternalProject_Add(ffmpeg_external
        URL "${FFMPEG_PREBUILT_URL}"
        DOWNLOAD_EXTRACT_TIMESTAMP TRUE
        PREFIX "${FFMPEG_PREFIX_DIR}"
        CONFIGURE_COMMAND ""
        BUILD_COMMAND ""
        INSTALL_COMMAND ""
        BUILD_BYPRODUCTS
        "${FFMPEG_LIB_DIR}/avcodec.lib"
        "${FFMPEG_LIB_DIR}/avformat.lib"
        "${FFMPEG_LIB_DIR}/avutil.lib"
        "${FFMPEG_LIB_DIR}/swresample.lib"
        "${FFMPEG_LIB_DIR}/swscale.lib"
    )

    # Link against the import libraries shipped with the Windows DLLs.
    add_library(ffmpeg_avcodec STATIC IMPORTED GLOBAL)
    set_target_properties(ffmpeg_avcodec PROPERTIES
        IMPORTED_LOCATION "${FFMPEG_LIB_DIR}/avcodec.lib"
        INTERFACE_INCLUDE_DIRECTORIES "${FFMPEG_INCLUDE_DIR}"
    )
    add_dependencies(ffmpeg_avcodec ffmpeg_external)

    add_library(ffmpeg_avformat STATIC IMPORTED GLOBAL)
    set_target_properties(ffmpeg_avformat PROPERTIES
        IMPORTED_LOCATION "${FFMPEG_LIB_DIR}/avformat.lib"
        INTERFACE_INCLUDE_DIRECTORIES "${FFMPEG_INCLUDE_DIR}"
    )
    add_dependencies(ffmpeg_avformat ffmpeg_external)

    add_library(ffmpeg_avutil STATIC IMPORTED GLOBAL)
    set_target_properties(ffmpeg_avutil PROPERTIES
        IMPORTED_LOCATION "${FFMPEG_LIB_DIR}/avutil.lib"
        INTERFACE_INCLUDE_DIRECTORIES "${FFMPEG_INCLUDE_DIR}"
    )
    add_dependencies(ffmpeg_avutil ffmpeg_external)

    add_library(ffmpeg_swresample STATIC IMPORTED GLOBAL)
    set_target_properties(ffmpeg_swresample PROPERTIES
        IMPORTED_LOCATION "${FFMPEG_LIB_DIR}/swresample.lib"
        INTERFACE_INCLUDE_DIRECTORIES "${FFMPEG_INCLUDE_DIR}"
    )
    add_dependencies(ffmpeg_swresample ffmpeg_external)

    add_library(ffmpeg_swscale STATIC IMPORTED GLOBAL)
    set_target_properties(ffmpeg_swscale PROPERTIES
        IMPORTED_LOCATION "${FFMPEG_LIB_DIR}/swscale.lib"
        INTERFACE_INCLUDE_DIRECTORIES "${FFMPEG_INCLUDE_DIR}"
    )
    add_dependencies(ffmpeg_swscale ffmpeg_external)

    add_library(ffmpeg INTERFACE)
    target_link_libraries(ffmpeg INTERFACE
        ffmpeg_avcodec
        ffmpeg_avformat
        ffmpeg_avutil
        ffmpeg_swresample
        ffmpeg_swscale
        bcrypt
        secur32
        ws2_32
        user32
        advapi32
        ole32
        shell32
    )

    set(PS2X_FFMPEG_BIN_DIR "${FFMPEG_BIN_DIR}" CACHE INTERNAL "Directory containing FFmpeg runtime DLLs")
    set(PS2X_COPY_FFMPEG_DLLS_SCRIPT
        "${CMAKE_CURRENT_SOURCE_DIR}/cmake/CopyFfmpegDlls.cmake"
        CACHE INTERNAL
        "Script used to stage FFmpeg runtime DLLs beside executables")
else()
    find_package(PkgConfig REQUIRED)
    pkg_check_modules(FFMPEG REQUIRED IMPORTED_TARGET GLOBAL
        libavcodec
        libavformat
        libavutil
        libswresample
        libswscale
    )

    add_library(ffmpeg INTERFACE)
    target_link_libraries(ffmpeg INTERFACE PkgConfig::FFMPEG)
endif()

function(ps2x_stage_ffmpeg_runtime_dlls target_name)
    if(NOT WIN32)
        return()
    endif()

    if(NOT PS2X_FFMPEG_BIN_DIR OR NOT PS2X_COPY_FFMPEG_DLLS_SCRIPT)
        return()
    endif()

    if(TARGET ffmpeg_external)
        add_dependencies(${target_name} ffmpeg_external)
    endif()

    add_custom_command(
        TARGET ${target_name} POST_BUILD
        COMMAND "${CMAKE_COMMAND}"
        "-Dsource_dir=${PS2X_FFMPEG_BIN_DIR}"
        "-Ddest_dir=$<TARGET_FILE_DIR:${target_name}>"
        -P "${PS2X_COPY_FFMPEG_DLLS_SCRIPT}"
        VERBATIM
    )
endfunction()

add_library(ps2_runtime STATIC
    src/lib/game_overrides.cpp
    src/lib/gs/ps2_gif_arbiter.cpp
    src/lib/ps2_audio.cpp
    src/lib/ps2_audio_vag.cpp
    src/lib/gs/ps2_gs_memory.cpp
    src/lib/gs/gs_frontend.cpp
    src/lib/gs/gs_stage.cpp # GOW-Port: GS frontend stage pipelined behind VU1.
    src/lib/gs/gs_display.cpp # GOW-Port: native display state + GS FINISH handshake (no-emulation step 5).
    src/lib/gs/gs_host_present.cpp # GOW-Port: same texture consumer in game and in the checks.
        # GOW-Port: OpenGL backend and queue of the SotC fork ac9efa0 (GPL-3.0).
    src/lib/gs/gs_threaded_backend.cpp
    src/lib/gs/gs_swizzle.cpp
    src/lib/gs/gpu/gs_gl.cpp
    src/lib/gs/gpu/gs_hw_backend.cpp

src/lib/gs/gpu/gs_texture_store.cpp
    src/lib/gs/gpu/gs_texture_replace.cpp # GOW-Port: PCSX2-compatible texture replacement packs (launcher: custom textures).
    src/lib/ps2_iop_host.cpp
    src/lib/ps2_memory.cpp
    src/lib/ps2_pad.cpp
    # GOW-Port: PC Port features on top of Waffle's runtime (gow_integration.cpp wires them in).
    src/lib/gow_integration.cpp
    src/lib/gow_retro_achievements.cpp
    src/lib/gow_retro_achievements_http.cpp
    src/lib/gow_sampling.cpp
    src/lib/gow_frame_metrics.cpp # GOW-Port: GOW_FRAME_DIAG guest-frame interval distribution
    src/lib/gow_cheats.cpp
    src/lib/gow_notice.cpp # GOW-Port: short host notices over the game
    src/lib/gow_save_backup.cpp # GOW-Port: dated copies of the memory card
    src/lib/gow_video.cpp
    src/lib/gow_targeting.cpp
    src/lib/gow_animation_cancel.cpp
    src/lib/gow_polling.cpp
    src/lib/gow_wait.cpp
    src/lib/ps2_audio_device.cpp # GOW-Port: sound output on a chosen device
    src/lib/ps2_pad_rumble_win.cpp # GOW-Port: Windows gamepad rumble (XInput, DualShock 4 / DualSense)
    src/lib/ps2_rom_device.cpp
    src/lib/ps2_vfs.cpp
    src/lib/ps2_runtime.cpp
    src/lib/ps2_vif1_interpreter.cpp
    src/lib/vu/ps2_vu1_core.cpp
    # GOW-Port: EE differential tester (PS2X_EE_DIFF) with PCSX2's EE interpreter and VU0 ops (GPL-3.0+).
    src/lib/ee_ref/ee_diff.cpp
    src/lib/ee_watch.cpp
    src/lib/ee_ref/eeref.cpp
    src/lib/ee_ref/R5900OpcodeImpl.cpp
    src/lib/ee_ref/R5900OpcodeTables.cpp
    src/lib/ee_ref/MMI.cpp
    src/lib/ee_ref/FPU.cpp
    src/lib/vu_pcsx2/pcsx2_vu0_cop2.cpp
    src/lib/vu_pcsx2/pcsx2_vu1_bridge.cpp
    src/lib/vu_pcsx2/VUops.cpp
    src/lib/vu_pcsx2/VUflags.cpp
    src/lib/vu_pcsx2/VU1microInterp.cpp
    src/lib/vu/ps2_vu1_upper.cpp
    src/lib/vu/ps2_vu1_lower.cpp
    src/lib/games_database.cpp
)

if(PS2X_ENABLE_RUNTIME_LOGS)
    target_compile_definitions(ps2_runtime PUBLIC
        PS2_RUNTIME_LOGS=1
    )
endif()

if(PS2X_ENABLE_AGRESSIVE_LOGS)
    target_compile_definitions(ps2_runtime PUBLIC
        AGRESSIVE_LOGS=1
        PS2_FUNCTION_LOG_TRACKER=1
    )
endif()

if(PS2X_ENABLE_IOP_RPC_TRACE)
    target_compile_definitions(ps2_runtime PUBLIC
        PS2X_ENABLE_IOP_RPC_TRACE=1
    )
else()
    target_compile_definitions(ps2_runtime PUBLIC
        PS2X_ENABLE_IOP_RPC_TRACE=0
    )
endif()

if(PS2X_STRICT_RETURN_DIAGNOSTICS)
    target_compile_definitions(ps2_runtime PUBLIC
        PS2X_STRICT_RETURN_DIAGNOSTICS=1
    )
endif()

target_compile_definitions(ps2_runtime PRIVATE
    PS2X_HAS_FFMPEG=$<BOOL:${PS2X_ENABLE_FFMPEG}>
)

file(GLOB_RECURSE KERNEL_SRC_FILES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/src/lib/Kernel/*.cpp"
)

target_sources(ps2_runtime PRIVATE
    ${KERNEL_SRC_FILES}
)

file(GLOB RUNNER_SRC_FILES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/src/runner/*.cpp"
)

# GOW-Port: older installers copied gow_overrides.cpp here; it is src/game now. Never build a stale copy twice.
list(FILTER RUNNER_SRC_FILES EXCLUDE REGEX "/src/runner/gow_[^/]*\\.cpp$")

set(RUNNER_MAIN_CPP "${CMAKE_CURRENT_SOURCE_DIR}/src/runner/main.cpp")
set(ROOT_MAIN_CPP "${CMAKE_CURRENT_SOURCE_DIR}/src/main.cpp")

if(EXISTS "${RUNNER_MAIN_CPP}")
    list(APPEND RUNNER_SRC_FILES "${RUNNER_MAIN_CPP}")
elseif(EXISTS "${ROOT_MAIN_CPP}")
    list(APPEND RUNNER_SRC_FILES "${ROOT_MAIN_CPP}")
endif()

# GOW-Port: the God of War overrides (src/game) wrap functions of the generated code and register themselves when the
# executable starts, so they belong to ps2EntryRunner and not to the ps2_runtime library. Each one is compiled on its
# own: they are few, and they must not share a unity-build translation unit with the generated functions.
file(GLOB GAME_SRC_FILES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/src/game/*.cpp"
)
set_source_files_properties(${GAME_SRC_FILES} PROPERTIES SKIP_UNITY_BUILD_INCLUSION ON)
list(APPEND RUNNER_SRC_FILES ${GAME_SRC_FILES})

# GOW-Port: register_functions.cpp (generated) is one constructor with a store per game function (~155,000, 12.8 MB) that
# runs once at start-up. Optimized inside a unity batch it was the longest unit of the build (clang -O3: 331 s; idea from
# gow-native-pc runtime patch 41, where -O1 alone took 9.8 s). It is built on its own, without the precompiled header
# (clang rejects a PCH made at another optimization level) and at a low level: nothing in it is hot.
set(PS2X_RUNNER_REGISTER_FILE "${CMAKE_CURRENT_SOURCE_DIR}/src/runner/register_functions.cpp")
if(EXISTS "${PS2X_RUNNER_REGISTER_FILE}")
    if(MSVC)
        set(PS2X_RUNNER_REGISTER_OPTIONS "/Od")
    else()
        set(PS2X_RUNNER_REGISTER_OPTIONS "-O1")
    endif()
    set_source_files_properties("${PS2X_RUNNER_REGISTER_FILE}" PROPERTIES
        SKIP_UNITY_BUILD_INCLUSION ON
        SKIP_PRECOMPILE_HEADERS ON
        COMPILE_OPTIONS "${PS2X_RUNNER_REGISTER_OPTIONS}")
endif()

add_executable(ps2EntryRunner
    ${RUNNER_SRC_FILES}
)

# GOW-Port: the target keeps its name (ninja ps2EntryRunner) but the file is gow-2005[.exe], with the Omega icon on Windows.
set_target_properties(ps2EntryRunner PROPERTIES OUTPUT_NAME gow-2005)
if(WIN32)
    target_sources(ps2EntryRunner PRIVATE resources/gow.rc)
endif()

if(PS2X_ENABLE_RUNNER_UNITY_BUILD)
    set_target_properties(ps2EntryRunner PROPERTIES
        UNITY_BUILD ON
        UNITY_BUILD_BATCH_SIZE "${PS2X_RUNNER_UNITY_BUILD_BATCH_SIZE}"
    )
endif()

if(PS2X_ENABLE_RUNNER_PCH)
    set(PS2X_RUNNER_PCH_HEADERS
        "${CMAKE_CURRENT_SOURCE_DIR}/include/ps2_runtime_macros.h"
        "${CMAKE_CURRENT_SOURCE_DIR}/include/ps2_runtime.h"
    )

    foreach(PS2X_RUNNER_GENERATED_HEADER IN ITEMS
        "${CMAKE_CURRENT_SOURCE_DIR}/include/ps2_recompiled_functions.h"
        "${CMAKE_CURRENT_SOURCE_DIR}/include/ps2_recompiled_stubs.h")
        if(EXISTS "${PS2X_RUNNER_GENERATED_HEADER}")
            list(APPEND PS2X_RUNNER_PCH_HEADERS "${PS2X_RUNNER_GENERATED_HEADER}")
        endif()
    endforeach()

    list(APPEND PS2X_RUNNER_PCH_HEADERS
        "${CMAKE_CURRENT_SOURCE_DIR}/include/ps2_syscalls.h"
        "${CMAKE_CURRENT_SOURCE_DIR}/include/ps2_stubs.h"
    )

    target_precompile_headers(ps2EntryRunner PRIVATE ${PS2X_RUNNER_PCH_HEADERS})
endif()

if(PS2X_DEFAULT_BOOT_ELF_DEFINE)
    target_compile_definitions(ps2EntryRunner PRIVATE
        PS2X_DEFAULT_BOOT_ELF="${PS2X_DEFAULT_BOOT_ELF_DEFINE}"
    )
endif()

if(MSVC)
    target_compile_options(ps2EntryRunner PRIVATE /FS)
endif()

target_include_directories(ps2_runtime PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/src/lib/Kernel
)
# GOW-Port: single-header libraries of gs_texture_replace.cpp (xxHash BSD-2, bcdec MIT/Unlicense, stb_image MIT/PD).
target_include_directories(ps2_runtime PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party/xxhash
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party/bcdec
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party/stb
)
# GOW-Port (Linux): ee_ref/common/BitUtils.h includes "common/Pcsx2Defs.h" relative to ee_ref/; MSVC finds it through
# the including file's folder, GCC/Clang do not. Only for the ee_ref sources (its R5900.h and Common.h
# must not shadow those of vu_pcsx2).
set_source_files_properties(
    src/lib/ee_ref/ee_diff.cpp src/lib/ee_ref/eeref.cpp src/lib/ee_ref/R5900OpcodeImpl.cpp
    src/lib/ee_ref/R5900OpcodeTables.cpp src/lib/ee_ref/MMI.cpp src/lib/ee_ref/FPU.cpp
    PROPERTIES INCLUDE_DIRECTORIES "${CMAKE_CURRENT_SOURCE_DIR}/src/lib/ee_ref")

target_link_libraries(ps2_runtime PUBLIC ps2_host_backend ffmpeg libmpeg2 ps2_iop rcheevos)
if(WIN32)
    # GOW-Port: the GS thread's own WGL.
    target_link_libraries(ps2_runtime PRIVATE opengl32 gdi32 winhttp bcrypt winmm psapi)
endif()
target_link_libraries(ps2EntryRunner
    PRIVATE
    ps2_runtime
)

# GOW-Port: MSVC options only for the VU1 microcode (the generated one and the core of ps2_vu1_*.cpp). /arch:AVX2 requires a
# CPU with AVX2 (the installer builds on the user's PC and can check it) and does not enable FMA by itself: the
# VU files carry fp_contract(off) and the rounding is checked with GOW_VU1N_RECORD/REPLAY (the output must be bit-for-bit identical).
option(PS2X_VU1_AVX2 "MSVC: build the VU1 microcode with /arch:AVX2 (requires a CPU with AVX2)" OFF)
set(PS2X_VU1_MSVC_OPTIONS "/bigobj")
if(MSVC AND PS2X_VU1_AVX2)
    list(APPEND PS2X_VU1_MSVC_OPTIONS "/arch:AVX2")
    set_source_files_properties(
        src/lib/vu/ps2_vu1_core.cpp src/lib/vu/ps2_vu1_lower.cpp src/lib/vu/ps2_vu1_upper.cpp
        PROPERTIES COMPILE_OPTIONS "/arch:AVX2")
endif()

# GOW-Port: compiled VU1 microcode. PS2X_VU1_MICROCODE_DIR is a local folder with micro memories of
# 16 KB capturadas con GOW_VU1_CAPTURE; PS2X_VU1_GENERATOR_SOURCE es tools/vu1/generate_vu1.cpp. El C++
# generated code is linked into the executable and the dispatcher uses it instead of the interpreter for the pairs that
# match. It is game data: it is generated in the build folder and not published.
set(PS2X_VU1_MICROCODE_DIR "" CACHE PATH "Captured VU1 micro-memories (empty: interpreter only)")
set(PS2X_VU1_GENERATOR_SOURCE "" CACHE FILEPATH "C++ generator for VU1 (RustPortPS2/native/tools/vu1/generate_vu1.cpp)")
set(PS2X_VU1_MAX_MEMORY_MB "0" CACHE STRING "VU1 generator RAM budget in MiB (0: unlimited)")
if(PS2X_VU1_MICROCODE_DIR AND PS2X_VU1_GENERATOR_SOURCE)
    file(GLOB PS2X_VU1_IMAGES CONFIGURE_DEPENDS "${PS2X_VU1_MICROCODE_DIR}/*.bin")
    if(PS2X_VU1_IMAGES)
        add_executable(ps2x_vu1_generator "${PS2X_VU1_GENERATOR_SOURCE}")
        target_link_libraries(ps2x_vu1_generator PRIVATE ps2_runtime)
        set(PS2X_VU1_GENERATED_DIR "${CMAKE_CURRENT_BINARY_DIR}/vu1_generated")
        set(PS2X_VU1_GENERATED_PARTS)
        foreach(part RANGE 0 12)
            list(APPEND PS2X_VU1_GENERATED_PARTS "${PS2X_VU1_GENERATED_DIR}/program${part}.cpp")
        endforeach()
        add_custom_command(OUTPUT ${PS2X_VU1_GENERATED_PARTS}
            COMMAND "${CMAKE_COMMAND}" -E make_directory "${PS2X_VU1_GENERATED_DIR}"
            # GOW-Port: the folder, not each file: thousands of paths exceed Windows' command line.
            COMMAND "$<TARGET_FILE:ps2x_vu1_generator>" "${PS2X_VU1_GENERATED_DIR}" --max-memory-mb "${PS2X_VU1_MAX_MEMORY_MB}" "${PS2X_VU1_MICROCODE_DIR}"
            DEPENDS ps2x_vu1_generator ${PS2X_VU1_IMAGES}
            VERBATIM)
        target_sources(ps2EntryRunner PRIVATE ${PS2X_VU1_GENERATED_PARTS})
        # GOW-Port: only the generated sources need this path; adding it to the target rebuilds ~800 game units.
        set_source_files_properties(${PS2X_VU1_GENERATED_PARTS} PROPERTIES
            SKIP_UNITY_BUILD_INCLUSION ON
            SKIP_PRECOMPILE_HEADERS ON
            INCLUDE_DIRECTORIES "${CMAKE_CURRENT_SOURCE_DIR}/src/lib/vu")
        if(MSVC)
            set_source_files_properties(${PS2X_VU1_GENERATED_PARTS} PROPERTIES COMPILE_OPTIONS "${PS2X_VU1_MSVC_OPTIONS}")
        endif()
        # GOW-Port: native lean backend (ps2-tools vu1n-gen): straight-line blocks with immediate semantics.
        set(PS2X_TOOLS_EXE "" CACHE FILEPATH "ps2-tools executable (RustPortPS2/crates/ps2-tools): generates the native VU1 blocks")
        if(NOT PS2X_TOOLS_EXE OR NOT EXISTS "${PS2X_TOOLS_EXE}")
            message(STATUS "Native VU1 blocks: PS2X_TOOLS_EXE is not set, the exact VU1 engine is used for every block")
        else()
            set(PS2X_VU1N_DIR "${CMAKE_CURRENT_BINARY_DIR}/vu1n_generated")
            set(PS2X_VU1N_PARTS)
            foreach(part RANGE 0 5)
                list(APPEND PS2X_VU1N_PARTS "${PS2X_VU1N_DIR}/vu1n_part${part}.cpp")
            endforeach()
            set(PS2X_VU1N_HEADS "${PS2X_VU1_MICROCODE_DIR}/../vu1n_heads.txt")
            get_filename_component(PS2X_VU1N_HEADS "${PS2X_VU1N_HEADS}" ABSOLUTE)
            set(PS2X_VU1N_HEADS_DEP)
            if(EXISTS "${PS2X_VU1N_HEADS}")
                set(PS2X_VU1N_HEADS_DEP "${PS2X_VU1N_HEADS}")
            endif()
            add_custom_command(OUTPUT ${PS2X_VU1N_PARTS}
                COMMAND "${PS2X_TOOLS_EXE}" vu1n-gen "${PS2X_VU1N_DIR}" "${PS2X_VU1_MICROCODE_DIR}" --heads "${PS2X_VU1N_HEADS}" --parts 6
                DEPENDS "${PS2X_TOOLS_EXE}" ${PS2X_VU1_IMAGES} ${PS2X_VU1N_HEADS_DEP}
                VERBATIM)
            target_sources(ps2EntryRunner PRIVATE ${PS2X_VU1N_PARTS})
            set_source_files_properties(${PS2X_VU1N_PARTS} PROPERTIES
                SKIP_UNITY_BUILD_INCLUSION ON
                SKIP_PRECOMPILE_HEADERS ON
                INCLUDE_DIRECTORIES "${CMAKE_CURRENT_SOURCE_DIR}/src/lib/vu")
            if(MSVC)
                set_source_files_properties(${PS2X_VU1N_PARTS} PROPERTIES COMPILE_OPTIONS "${PS2X_VU1_MSVC_OPTIONS}")
            endif()
        endif()
        list(LENGTH PS2X_VU1_IMAGES PS2X_VU1_IMAGE_COUNT)
        message(STATUS "Compiled VU1: ${PS2X_VU1_IMAGE_COUNT} micro-memories in ${PS2X_VU1_MICROCODE_DIR}")
    endif()
endif()

if(PS2X_ENABLE_DEBUG_UI)
    target_sources(ps2EntryRunner PRIVATE
        src/lib/ps2_debug_panel.cpp
    )
    target_link_libraries(ps2EntryRunner PRIVATE rlImGui imgui)
    target_compile_definitions(ps2EntryRunner PRIVATE PS2X_ENABLE_DEBUG_UI=1)
endif()

ps2x_stage_ffmpeg_runtime_dlls(ps2EntryRunner)

if(CMAKE_BUILD_TYPE STREQUAL "Release" OR CMAKE_BUILD_TYPE STREQUAL "RelWithDebInfo")
    EnableFastReleaseMode(ps2_runtime)
    EnableFastReleaseMode(ps2EntryRunner)

    if(WIN32 AND NOT PS2X_SHOW_WINDOWS_CONSOLE)
        if(MSVC)
            target_link_options(ps2EntryRunner PRIVATE
                $<$<OR:$<CONFIG:Release>,$<CONFIG:RelWithDebInfo>>:/SUBSYSTEM:WINDOWS>
                $<$<OR:$<CONFIG:Release>,$<CONFIG:RelWithDebInfo>>:/ENTRY:mainCRTStartup>
            )
        elseif(MINGW)
            target_link_options(ps2EntryRunner PRIVATE
                $<$<OR:$<CONFIG:Release>,$<CONFIG:RelWithDebInfo>>:-mwindows>
            )
        endif()
    endif()
endif()

# GOW-Port: VIF1/VU1 test bench (tools/render/replay_vif_chain.cpp), outside "all". It replays the VIF1 chain of a frame
# captured with GOW_VIF_CAPTURE over VIF1/VU1/GS, without the EE and with the game's compiled microcode. It prints ms per frame, a
# fingerprint of everything that goes out to the GS and (GOW_REPLAY_HASHES) a fingerprint of the VU1 state per launch.
set(PS2X_REPLAY_VIF_SRC "${CMAKE_CURRENT_SOURCE_DIR}/../tools/render/replay_vif_chain.cpp")
if(EXISTS "${PS2X_REPLAY_VIF_SRC}")
    add_executable(replay_vif_chain EXCLUDE_FROM_ALL "${PS2X_REPLAY_VIF_SRC}")
    target_link_libraries(replay_vif_chain PRIVATE ps2_runtime)
    if(PS2X_VU1_GENERATED_PARTS)
        target_sources(replay_vif_chain PRIVATE ${PS2X_VU1_GENERATED_PARTS} ${PS2X_VU1N_PARTS})
    endif()
    if(MSVC)
        target_compile_options(replay_vif_chain PRIVATE /FS)
        # 64 MB stack: processVIF1Data runs on the main thread and the compiled VU1 uses large frames.
        target_link_options(replay_vif_chain PRIVATE "/FORCE:MULTIPLE" "/STACK:67108864")
    endif()
endif()

# GOW-Port: gow-2005.map next to the executable (function names for the sampling, ps2-dev sampling-win).
option(PS2X_MAP "MSVC: write gow-2005.map when linking" OFF)
if(MSVC AND PS2X_MAP)
    target_link_options(ps2EntryRunner PRIVATE /MAP)
    if(TARGET replay_vif_chain)
        target_link_options(replay_vif_chain PRIVATE /MAP)
    endif()
endif()

# Work around WinAPI vs raylib symbol clash for CloseWindow on x64
if(MSVC)
    target_link_options(ps2EntryRunner PRIVATE "/FORCE:MULTIPLE")
endif()

install(TARGETS ps2_runtime ps2EntryRunner
    RUNTIME DESTINATION bin
    LIBRARY DESTINATION lib
    ARCHIVE DESTINATION lib
)

if(WIN32 AND PS2X_ENABLE_FFMPEG)
    install(DIRECTORY "${FFMPEG_BIN_DIR}/"
        DESTINATION bin
        FILES_MATCHING
        PATTERN "*.dll"
    )
endif()

install(DIRECTORY include/
    DESTINATION include
)

# GOW-Port include
target_include_directories(ps2EntryRunner PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/src/runner)
