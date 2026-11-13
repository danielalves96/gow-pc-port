#pragma once
// GOW-Port: GS frontend stage (docs/dev/PERFORMANCE.md "VU1 / GS frontend pipeline").
// With the GS worker on (PS2X_GS_WORKER=1), the worker runs VIF1 + VU1 and hands every GIF packet the arbiter
// emits to this stage, which runs the GS frontend (GS::processGIFPacket) on its own thread in the same order.
// VU1 work of packet N+1 then overlaps the GS frontend work of packet N instead of following it.
// The stage is part of the worker for synchronisation: PS2Memory's worker-idle wait also waits for the stage,
// so every EE access that synced with the worker still sees all queued GS work done.
// PS2X_GS_PIPELINE=0 keeps the GS frontend on the worker thread. Kept out of the precompiled header.
#include <cstdint>
#include <functional>

#include "runtime/gs/ps2_gif_arbiter.h"

namespace ps2_gs_stage
{
    using ProcessFn = std::function<void(GifPathId, const uint8_t *, uint32_t)>;

    // Starts the stage thread (once). `process` runs each packet on the stage thread; `afterBatch` runs there
    // after each batch of packets (used to wake the EE when the GS raised an interrupt).
    void start(ProcessFn process, std::function<void()> afterBatch);
    // PS2X_GS_PIPELINE=0: no stage thread; push() runs `process` at once on the calling thread (the GS worker), so the
    // markers PS2Memory pushes (display registers, early FINISH) still reach the GS frontend in order.
    void setDirect(ProcessFn process);
    void stop();
    bool running();
    bool onStageThread();

    // Queues a copy of the packet (blocks while too much is queued, so the worker cannot run far ahead).
    void push(GifPathId path, const uint8_t *data, uint32_t size);

    // Returns when every queued packet has been processed. No-op on the stage thread itself.
    // GOW-Port: true when it had to wait (the stage still had packets), for PS2X_GS_WORKER_DIAG.
    bool waitIdle();
}
