// GOW-Port: entry points of the God of War overrides (SCUS-97399). Each module (gow_*.cpp in this folder) defines the
// ones of its area; gow_overrides.cpp binds them to the game's addresses.
//
// Every override has the signature of a recompiled function. It calls the original for everything it does not
// replace, and when the call comes back with `ctx->pc` different from the entry (a resumption after an EE
// checkpoint) it calls the original without further ado.
#pragma once

#include "gow_guest.h"
#include <cstddef>
#include <string>

int gowLogicFps(); // lib/gow_integration.cpp: PC_LOGIC_FPS (60 unless 120, 144, 165 or 240 and no 30 FPS cap)
void gowNoteGameFlip(); // lib/gow_integration.cpp: the game swapped its display buffer
void gowSetHangReportExtra(void (*report)(PS2Runtime &)); // lib/gow_integration.cpp: more [gow-hang] lines (diagnostics)
void gowSetVBlankExtra(void (*callback)(uint8_t *rdram)); // lib/gow_integration.cpp: one more EE-thread callback per VBlank

namespace gow_game
{
    // ---- pad (gow_pad2.cpp, gow_pad_test.cpp, gow_warp.cpp) -------------------------------------------------------
    void gowPad2Init(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowPad2CreateSocket(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowPad2DeleteSocket(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowPad2GetState(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowPad2GetButtonProfile(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowPad2Read(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowVibGetProfile(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowVibSetActParam(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);

    // GOW_PAD_TEST=1: scripted input and captures; `state` is the 32-byte pad status the script edits.
    bool padSmokeTestEnabled();
    void padSmokeTest(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime, uint8_t (&state)[32]);

    // GOW_WARP: called from the pad read, once per frame. gowCurrentZone is the name of the zone Kratos is in.
    void gowWarpPoll(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    std::string gowCurrentZone(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);

    // ---- replacements of system calls (gow_system.cpp, gow_display.cpp) ---------------------------------------------
    void gowIpuInit(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowRand(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowCdReadDvdDualInfo(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowLoadStartModuleBuffer(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDelayThread(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowTimerSystemNoop(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void configureGowCdImage();

    void gowDisplaySetup(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowGsFinishHandler(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowGsFinishInstall(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowRenderServerDispatch(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);

    // ---- object guard and heap (gow_heap_guard.cpp) -----------------------------------------------------------------
    void gowOperatorNew(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowOperatorDelete(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowGuardTimelineChannels(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowGuardNodeDestructor(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowGuardCloneList(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowHeapZeroAlloc(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowFlashQueueDrain(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowFlashQueueAppend(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);

    // ---- movies (gow_movie.cpp) ---------------------------------------------------------------------------------------
    void gowSkipMovieLoad(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowSkipMovieStart(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowSkipMovieReady(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowSkipMovieNoop(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowMovieOverrideLoad(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowMovieStreamService(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowMovieVideoRingWrite(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowMovieNodata(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagBonusMovie(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagStreamRead(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    size_t movieOverrideCount(); // entries of GOW_MOVIE_OVERRIDE

    // ---- game logic rate (gow_logic_rate.cpp) ------------------------------------------------------------------------
    void gowLogicTimeStep(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);  // PC_LOGIC_FPS > 60 or GOW_HFPS_DIAG=1
    void gowLogicFrameWait(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime); // PC_LOGIC_FPS > 60

    // ---- diagnostics (gow_diag_*.cpp): each logs what it reads and then runs the original ---------------------------------
    void gowDiagPathSelect(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagAttachNode(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowSnd989SendCommand(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagSoundGroups(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowLoaderDiagInstall(PS2Runtime &runtime); // GOW_LOADER_DIAG=1: the level loader in the [gow-hang] report

    void gowDiagCalcWorldMatrix(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagController(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagBlend(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagFlashReady(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagAnimationTime(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagAnimationDuration(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);

    // GOW_MENU_DIAG=1 / GOW_NATIVE_CHEATS_MENU=1: bounded read-only observers of the original UI (gow_menu_diag.cpp).
    void gowDiagMenuMessages(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime); // 0x1B8A48 LoadMessages
    void gowDiagMenuShow(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);     // 0x1B9358 ShowPrompt
    void gowDiagMenuMovie(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);    // 0x1B7CE0 LoadMovie
    // GOW_NATIVE_CHEATS_MENU=1 (gow_menu_diag.cpp): Port Options extension. BuildMovie 0x14AE68 hands the game a movie with
    // the Port Options row and pages; the VBlank callback keeps the PS2_<cheat> HUD variables and the cheat engine in step.
    void gowNativeMenuBuildMovie(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowNativeMenuVBlank(uint8_t *rdram);

    void gowDiagFilteredCopy(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagInitUnpack(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagUpdateAddress(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagPrimContext(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagModelServer(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagGrobMaster(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagModelContext(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagProcessModel(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagModelClip(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime);
    void gowDiagPrimPoll(uint8_t *rdram, double seconds); // GOW_EE_PRIM_DIAG samples, called from the pad read
}
