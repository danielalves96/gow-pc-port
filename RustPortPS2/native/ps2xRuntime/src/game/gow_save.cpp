// GOW-Port: the moment a save is complete, and what the port does with it (a dated backup of the card, a notice).
//
// God of War saves through libmc2 (EE) over MC2_D/DBCMAN (IOP), not through the generic MCSERV calls, and the card
// image is written page by page: nothing at that level says "the save is finished". The game's own save state machine
// (0x22FE30) does: it writes the slot's payload with the whole-file writer 0x22E800 and then the 68-byte slot index with
// the same writer, from 0x2306B8. The writer is polled once per frame: it returns 0 while the transfer is pending and 1
// when it ended, with the game's error code at 0x2A1284 (0 = none) and the bytes transferred at 0x2A128C. The state
// machine moves on after that call, so a completed index write is seen exactly once per save.
//
// The wrapper only observes: it always runs the original and looks at the result when the original comes back to that
// call site. Any other caller, a pending transfer, an error, another slot or a resumption inside the original (the call
// did not come back to its caller) is left alone. GOW_SAVE_DIAG=1 prints every completed index write with the words it
// looked at; GOW_SAVE_BACKUP=0 turns the backup off.
#include "gow_game.h"
#include "gow_guest_functions.h"
#include "../lib/gow_notice.h"
#include "../lib/gow_save_backup.h"

#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <string>

namespace gow_game
{
    namespace
    {
        constexpr uint32_t kWriteFile = 0x0022E800u;    // whole-file writer (slot, buffer, offset, size)
        constexpr uint32_t kIndexCaller = 0x002306C0u;   // return address of the index write in the save state machine
        constexpr uint32_t kIndexSize = 0x44u;           // bytes of the slot index (68)
        constexpr uint32_t kError = 0x002A1284u, kBytes = 0x002A128Cu;

        std::atomic<uint32_t> g_completedSaves{0};

        bool backupEnabled()
        {
            static const bool on = [] { const char *v = std::getenv("GOW_SAVE_BACKUP"); return !v || v[0] != '0'; }();
            return on;
        }

        bool diagEnabled()
        {
            static const bool on = [] { const char *v = std::getenv("GOW_SAVE_DIAG"); return v && v[0] == '1'; }();
            return on;
        }

        void saveCompleted(uint32_t error, uint32_t bytes)
        {
            const uint32_t number = g_completedSaves.fetch_add(1u, std::memory_order_relaxed) + 1u;
            std::fprintf(stderr, "[gow-save] save complete (#%u, error=%u, bytes=%u)\n", number, error, bytes);
            if (!backupEnabled())
                return;
            const GowSaveBackupResult backup = gowSaveBackup("auto");
            const bool pt = gowNoticePortuguese();
            if (backup.ok)
            {
                std::fprintf(stderr, "[gow-save] backup kept in %s\n", backup.directory.c_str());
                gowNoticePost(pt ? "Backup do save" : "Save backup",
                              pt ? "Cópia do cartão guardada no histórico." : "A copy of the card was kept in the history.", 6.0);
            }
            else
            {
                std::fprintf(stderr, "[gow-save] backup failed: %s\n", backup.error.c_str());
                gowNoticePost(pt ? "Backup do save" : "Save backup",
                              pt ? "Não foi possível guardar a cópia do cartão." : "The card copy could not be kept.", 8.0, true);
            }
        }
    }

    uint32_t gowSaveCompletedCount()
    {
        return g_completedSaves.load(std::memory_order_relaxed);
    }

    void gowSaveWriteFile(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const bool entry = ctx->pc == kWriteFile;
        const uint32_t caller = GPR_U32(ctx, 31), slot = GPR_U32(ctx, 4), size = GPR_U32(ctx, 7);
        const bool indexWrite = entry && caller == kIndexCaller && slot == 0u && size == kIndexSize;
        sub_0022E800_0x22e800(rdram, ctx, runtime);
        if (!indexWrite || ctx->pc != caller || GPR_U32(ctx, 2) != 1u)
            return;
        const uint32_t error = readGuest32(rdram, kError), bytes = readGuest32(rdram, kBytes);
        if (diagEnabled())
            std::fprintf(stderr, "[gow-save] index write done: error=%u bytes=%u\n", error, bytes);
        if (error == 0u && bytes == kIndexSize)
            saveCompleted(error, bytes);
    }
}
