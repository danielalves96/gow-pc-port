// GOW-Port: manual links for God of War (SCUS-97399): binds each override (gow_*.cpp in this folder) to the address of
// the game function it replaces or wraps.
#include "gow_game.h"
#include "gow_gs_replay.h" // GOW-Port: optional, bounded capture to compare backends.
#include "game_overrides.h"
#include "runtime/ee_scheduler.h"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <thread>

namespace
{
    using namespace gow_game;

    // GOW-Port: GOW_THREAD_WATCH=<s> (debug): every <s> seconds a host thread prints the EE threads (status, wait,
    // pc, ra) from the scheduler's snapshot, so a guest that blocks for good (no frames, no pad polls, where
    // GOW_THREAD_DUMP never fires) still shows where each thread waits
    void watchThreads(PS2Runtime &runtime)
    {
        if (const char *watch = std::getenv("GOW_THREAD_WATCH"))
        {
            const double period = std::max(1.0, std::atof(watch));
            std::thread([&runtime, period] {
                for (;;)
                {
                    std::this_thread::sleep_for(std::chrono::duration<double>(period));
                    const EeKernelSnapshot snapshot = runtime.eeScheduler().snapshot();
                    std::fprintf(stderr, "[gow-threads] watch running=%d cycle=%llu\n", snapshot.runningThreadId,
                                 static_cast<unsigned long long>(snapshot.eeCycle));
                    for (const EeThreadSnapshot &t : snapshot.threads)
                        std::fprintf(stderr, "[gow-threads]   id=%d prio=%d status=%d wait=%d/%d pc=%08x ra=%08x\n", t.id,
                                     t.currentPriority, int(t.status), int(t.waitReason), t.waitId, t.pc, t.ra);
                    // the movie player struct (*0x304F20) words +0xE8..+0x15C, when a movie is loaded
                    const uint8_t *ram = runtime.memory().getRDRAM();
                    const auto word = [ram](uint32_t a) { uint32_t v; std::memcpy(&v, ram + (a & 0x01FFFFFFu), 4); return v; };
                    if (const uint32_t movie = word(0x304F20u); movie != 0u)
                    {
                        std::string text;
                        for (uint32_t off = 0xE8u; off < 0x160u; off += 4u)
                        {
                            char item[24];
                            std::snprintf(item, sizeof(item), " +%x=%x", off, word(movie + off));
                            text += item;
                        }
                        std::fprintf(stderr, "[gow-threads]   movie=%x%s\n", movie, text.c_str());
                    }
                }
            }).detach();
        }
    }

    void applyGowOverrides(PS2Runtime &runtime)
    {
        watchThreads(runtime);
        configureGowCdImage();
        gow_gs_replay::configure(runtime.memory(),runtime.gs());
        // The runtime's internal heap goes below the ELF (free memory) so it doesn't overlap the game's own heap,
        // which it creates with SetupHeap at the end of .bss (patches/ps2recomp-heap.patch).
        runtime.setPrivateGuestHeap(0x000A0000u, 0x000FF000u);
        runtime.replaceFunction(0x00279600u, gowIpuInit);
        runtime.replaceFunction(0x0028C0C8u, gowRand);
        runtime.replaceFunction(0x0014C278u, gowOperatorNew);         // object guard (heap_guard)
        runtime.replaceFunction(0x0014C2F0u, gowOperatorDelete);
        runtime.replaceFunction(0x00147520u, gowGuardTimelineChannels);
        runtime.replaceFunction(0x0023B6A0u, gowGuardNodeDestructor);
        runtime.replaceFunction(0x00149340u, gowGuardCloneList);
        if (std::getenv("GOW_FLASHQ_WATCH"))
        {
            runtime.replaceFunction(0x00146158u, gowFlashQueueDrain);
            runtime.replaceFunction(0x001460C0u, gowFlashQueueAppend);
        }
        if (std::getenv("GOW_CTRL_DIAG"))
            runtime.replaceFunction(0x00116EF0u, gowDiagController);
        if (std::getenv("GOW_BLEND_DIAG"))
            runtime.replaceFunction(0x00110E40u, gowDiagBlend);
        // GOW-Port: observers of the original UI (HUD object, message bank, Flash movies); read-only, always call the original.
        const char *menuDiag = std::getenv("GOW_MENU_DIAG");
        const char *menuExt = std::getenv("GOW_NATIVE_CHEATS_MENU");
        if ((menuDiag && std::strcmp(menuDiag, "1") == 0) || (menuExt && std::strcmp(menuExt, "1") == 0))
        {
            runtime.replaceFunction(0x001B8A48u, gowDiagMenuMessages);
            runtime.replaceFunction(0x001B9358u, gowDiagMenuShow);
            runtime.replaceFunction(0x001B7CE0u, gowDiagMenuMovie);
            if (menuExt && std::strcmp(menuExt, "1") == 0)
            {
                runtime.replaceFunction(0x0014AE68u, gowNativeMenuBuildMovie);
                gowSetVBlankExtra(gowNativeMenuVBlank);
            }
            std::fprintf(stderr, "[gow-menu] observers on; the Options extension is %s\n",
                         menuExt && std::strcmp(menuExt, "1") == 0 ? "installed (Port Options and Cheats in the pause menu, see docs/dev/NATIVE_MENU_RESEARCH.md)" : "not requested");
        }
        if (std::getenv("GOW_HEAP_ZERO"))
            runtime.replaceFunction(0x0013DC78u, gowHeapZeroAlloc);
        if (std::getenv("GOW_SKEL_DIAG") || std::getenv("GOW_SKEL_OBJECT") || std::getenv("GOW_SKEL_ANIM"))
            runtime.replaceFunction(0x00157A60u, gowDiagCalcWorldMatrix);
        if (std::getenv("GOW_PATH_DIAG") || std::getenv("GOW_ANM_DIAG") || std::getenv("GOW_FAST_BOOT"))
        {
            runtime.replaceFunction(0x001B2390u, gowDiagFlashReady);
            runtime.replaceFunction(0x00100C50u, gowDiagAnimationTime);
            runtime.replaceFunction(0x00100BF0u, gowDiagAnimationDuration);
        }
        if (const char *skip = std::getenv("GOW_SKIP_FMV"); skip && std::strcmp(skip, "1") == 0)
        {
            runtime.replaceFunction(0x00188CA0u, gowSkipMovieLoad);
            runtime.replaceFunction(0x00188E80u, gowSkipMovieStart);
            runtime.replaceFunction(0x00188ED8u, gowSkipMovieReady);
            runtime.replaceFunction(0x00188EF0u, gowSkipMovieReady);
            runtime.replaceFunction(0x00188F20u, gowSkipMovieNoop);
        }
        else
        {
            runtime.replaceFunction(0x00188B80u, gowMovieStreamService);
            runtime.replaceFunction(0x0018A788u, gowMovieVideoRingWrite);
            runtime.replaceFunction(0x0018AAA8u, gowMovieNodata);
            if (std::getenv("GOW_MOVIE_OVERRIDE") || std::getenv("GOW_MPEG_DIAG"))
                runtime.replaceFunction(0x00188CA0u, gowMovieOverrideLoad);
            if (std::getenv("GOW_BONUS_MOVIE") || movieOverrideCount() > 1)
                runtime.replaceFunction(0x001BA4F8u, gowDiagBonusMovie);
            if (std::getenv("GOW_SND_CALLLOG"))
                runtime.replaceFunction(0x00175148u, gowDiagSoundGroups);
            if (std::getenv("GOW_MPEG_DIAG"))
                runtime.replaceFunction(0x0017AFE0u, gowDiagStreamRead);
        }
        if (const char *diag = std::getenv("GOW_LOADER_DIAG"); diag && std::strcmp(diag, "1") == 0)
            gowLoaderDiagInstall(runtime);
        // game logic rate (gow_logic_rate.cpp): PC_LOGIC_FPS > 60 paces the frame at N Hz, off by default
        // bound whenever the menus can change the FPS limit (GOW_NATIVE_CHEATS_MENU=1), not only when the game starts above 60: the
        // wrappers run the original when the limit is 60
        const char *frameMenu = std::getenv("GOW_NATIVE_CHEATS_MENU");
        const bool frameMenuOn = frameMenu && std::strcmp(frameMenu, "1") == 0;
        const int logicFps = frameMenuOn ? std::max(gowLogicFps(), 120) : gowLogicFps();
        if (logicFps > 60)
        {
            runtime.replaceFunction(0x0017A1D0u, gowLogicFrameWait); // frame end's VBlank wait
            std::fprintf(stderr, "[gow-hfps] game logic at %d Hz (experimental, PC_LOGIC_FPS)\n", gowLogicFps());
        }
        if (const char *diag = std::getenv("GOW_HFPS_DIAG"); logicFps > 60 || (diag && std::strcmp(diag, "1") == 0))
            runtime.replaceFunction(0x0017D778u, gowLogicTimeStep); // per-frame time step
        if (std::getenv("GOW_RENDER_DIAG"))
            for (const uint32_t address : {0x001262C8u, 0x00126310u, 0x0012633Cu, 0x00126368u,
                                           0x00126428u, 0x00126478u, 0x00126528u})
                runtime.replaceFunction(address, gowDiagFilteredCopy);
        if (const char *diag=std::getenv("GOW_EE_PRIM_DIAG"); diag && std::strcmp(diag,"1")==0)
        {
            runtime.replaceFunction(0x00141350u, gowDiagInitUnpack);
            runtime.replaceFunction(0x001417D8u, gowDiagUpdateAddress);
            runtime.replaceFunction(0x00141B78u, gowDiagPrimContext);
        }
        if (const char *diag = std::getenv("GOW_MODEL_DIAG"); diag && std::strcmp(diag, "1") == 0)
        {
            runtime.replaceFunction(0x00159C58u, gowDiagModelServer);
            runtime.replaceFunction(0x001511F0u, gowDiagGrobMaster);
            runtime.replaceFunction(0x00159878u, gowDiagModelContext);
            runtime.replaceFunction(0x00157A60u, gowDiagProcessModel);
            runtime.replaceFunction(0x00169120u, gowDiagModelClip);
        }
        runtime.replaceFunction(0x00180E50u, gowDiagPathSelect);
        runtime.replaceFunction(0x00180D08u, gowDiagAttachNode);
        runtime.replaceFunction(0x0017A830u, gowRenderServerDispatch);
        runtime.replaceFunction(0x00183360u, gowDisplaySetup);
        runtime.replaceFunction(0x00182DD8u, gowGsFinishHandler);
        runtime.replaceFunction(0x00182E88u, gowGsFinishInstall);
        runtime.replaceFunction(0x00294A30u, gowDelayThread);     // EE timer 2: native DelayThread
        runtime.replaceFunction(0x002998C0u, gowTimerSystemNoop); // InitTimer
        runtime.replaceFunction(0x00299A20u, gowTimerSystemNoop); // StartTimerSystemTime
        runtime.replaceFunction(0x0027B7E8u, gowPad2Init);
        runtime.replaceFunction(0x0027B828u, gowPad2Init);
        runtime.replaceFunction(0x0027B890u, gowPad2CreateSocket);
        runtime.replaceFunction(0x0027B998u, gowPad2DeleteSocket);
        runtime.replaceFunction(0x0027B9F0u, gowPad2Read);
        runtime.replaceFunction(0x0027BAC8u, gowPad2GetButtonProfile);
        runtime.replaceFunction(0x0027BB98u, gowPad2GetState);
        runtime.replaceFunction(0x0027BF90u, gowVibGetProfile);
        runtime.replaceFunction(0x0027C010u, gowVibSetActParam);
        // The recompiler discards these two functions because they start in the delay slot
        // of a stray "jr ra" from the previous function.
        const bool a = ps2_game_overrides::bindAddressHandler(runtime, 0x00296C48u, "sceSifInitRpc");
        const bool b = ps2_game_overrides::bindAddressHandler(runtime, 0x00294990u, "iWakeupThread");
        // No handler in the runtime: the original version waits forever for the IOP's CDVD.
        const bool c = runtime.replaceFunction(0x0027AB00u, gowCdReadDvdDualInfo);
        const bool d = runtime.replaceFunction(0x0026BF28u, gowSnd989SendCommand);
        const bool e = runtime.replaceFunction(0x00298CE8u, gowLoadStartModuleBuffer);
        std::fprintf(stderr, "[gow-override] sceSifInitRpc=%d iWakeupThread=%d sceCdReadDvdDualInfo=%d snd989=%d modbuf=%d\n",
                     a, b, c, d, e);
    }
}

PS2_REGISTER_GAME_OVERRIDE("God of War (SCUS-97399)", "SCUS_973.99", 0x00100008u, 0u, applyGowOverrides)
