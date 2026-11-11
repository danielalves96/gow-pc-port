// GOW-Port: object guard for the game's operator new/delete and the UI timeline containers (heap_guard), the
// GOW_HEAP_ZERO test and the GOW_FLASHQ_WATCH logging of the Flash UI player's action queue.

#include "gow_game.h"
#include "gow_guest_functions.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <unordered_map>

void gowDumpBranchRing(const char *why); // GOW-Port: ps2_runtime.cpp

namespace gow_game
{
    namespace
    {
        // GOW-Port: object guard for the game's operator new (0x14C278, fixed-block pools) and delete (0x14C2F0).
        // The Flash-style UI player (timeline nodes, vtable 0x2BF4C0: 0x147520 evaluates a node's channels, 0x23B6A0 is
        // the node destructor, 0x149340 empties a node's duplicated-clip list) can be left holding a child that was already
        // deleted; the boss/enemy meter HUD (HydraMeter, "_level0/:CheckForGlow2") did it in the user's runs (ship after the
        // Hydra fights, Cliffs combat). Deleting that child again runs a destructor through whatever now occupies the block
        // (a string: jump to 0x3F; or a zeroed list: jump to 0) and frees the block a second time, which links it into the
        // pool's free list twice and hands it to two owners later. The guard tracks every block of operator new, ignores a
        // second delete of the same block, and drops a stale or NULL child from those three containers before the game
        // deletes it. Each case is logged ([gow-heapguard], the first ones with the last guest branches) to find the cause.
        namespace heap_guard
        {
            struct Block
            {
                bool live = true;
                uint32_t freedBy = 0; // caller of the delete that freed it
            };
            std::unordered_map<uint32_t, Block> blocks;
            unsigned reports = 0;

            void report(const char *what, uint32_t object, uint32_t where)
            {
                if (reports >= 200u)
                    return;
                ++reports;
                const auto it = blocks.find(object);
                std::fprintf(stderr, "[gow-heapguard] %s object=0x%x at=0x%x freed-by=0x%x\n", what, object, where,
                             it == blocks.end() ? 0u : it->second.freedBy);
                if (reports <= 3u)
                    gowDumpBranchRing(what);
            }

            bool stale(uint32_t object)
            {
                const auto it = blocks.find(object);
                return it != blocks.end() && !it->second.live;
            }

            // A child the game may delete: a live block whose virtual destructor (vtable at +4, entry +0xC) is game code.
            bool deletable(const uint8_t *rdram, PS2Runtime *runtime, uint32_t object)
            {
                if (object == 0u || object >= 0x02000000u || (object & 3u) != 0u || stale(object))
                    return false;
                const uint32_t vtable = readGuest32(rdram, object + 4u);
                if (vtable < 0x00100000u || vtable >= 0x02000000u || (vtable & 3u) != 0u)
                    return false;
                return runtime->hasFunction(readGuest32(rdram, vtable + 0xCu));
            }
        }

        // GOW_FLASHQ_WATCH=<hex block> (debug): logs new/delete of that block and every Flash queue append/drain holding it
        uint32_t flashqWatch()
        {
            static const uint32_t watch = std::getenv("GOW_FLASHQ_WATCH") ? std::strtoul(std::getenv("GOW_FLASHQ_WATCH"), nullptr, 16) : 0u;
            return watch;
        }
    }

    // GOW-Port: GOW_HEAP_ZERO=1 zero-fills every block returned by the game heap allocator 0x13DC78 (a0 heap, a1 bytes),
    // to test whether uninitialised (stale) heap data, which differs from PCSX2 after the allocation-order drift,
    // reaches the animation code.
    void gowHeapZeroAlloc(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t bytes = GPR_U32(ctx, 5);
        sub_0013DC78_0x13dc78(rdram, ctx, runtime);
        const uint32_t block = GPR_U32(ctx, 2) & 0x01FFFFFFu;
        if (block && bytes && bytes < 0x01000000u && block + bytes <= 0x02000000u)
            std::memset(rdram + block, 0, bytes);
    }

    void gowOperatorNew(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t ra = GPR_U32(ctx, 31);
        sub_0014C278_0x14c278(rdram, ctx, runtime);
        // A recompiled function hands control back early at an EE checkpoint (ctx->pc = where to resume) and is then
        // resumed through the original's own slots, so v0 is only the new block once the call has returned to ra.
        // The allocator (0x13DA10 pools, strcmp) never waits: finish it here instead of reading a stale v0, which left
        // reallocated blocks marked free (false "second delete" reports, and the guard dropping live children).
        for (int steps = 0; ctx->pc != ra && ctx->pc != 0u && steps < 100000 && !runtime->isStopRequested(); ++steps)
            runtime->lookupFunction(ctx->pc)(rdram, ctx, runtime);
        if (ctx->pc != ra)
            return;
        if (const uint32_t block = GPR_U32(ctx, 2))
        {
            heap_guard::blocks[block] = {};
            if (block == flashqWatch())
                std::fprintf(stderr, "[gow-flashq] new 0x%x ra=0x%x\n", block, ra);
        }
    }

    void gowOperatorDelete(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t block = GPR_U32(ctx, 4), ra = GPR_U32(ctx, 31);
        // delete of NULL does nothing. It is reached after a skipped virtual destructor call on a NULL UI node
        // ([gow-badcall], issue #20: 0x149340 makes that call at 0x149390 and calls delete at 0x1493BC).
        if (block == 0u && ctx->pc == 0x0014C2F0u)
        {
            ctx->pc = ra;
            return;
        }
        // 0x143C78 (string delete) keeps its caller at 0($sp)
        const uint32_t caller = ra == 0x00143C8Cu ? readGuest32(rdram, GPR_U32(ctx, 29)) : ra;
        if (block == flashqWatch())
            std::fprintf(stderr, "[gow-flashq] delete 0x%x by 0x%x\n", block, caller);
        const auto it = heap_guard::blocks.find(block);
        if (it != heap_guard::blocks.end())
        {
            if (!it->second.live)
            {
                heap_guard::report("second delete ignored", block, caller);
                ctx->pc = ra;
                return;
            }
            it->second = {false, caller};
        }
        sub_0014C2F0_0x14c2f0(rdram, ctx, runtime);
    }

    // GOW_FLASHQ_WATCH=<hex block> (debug) also wraps the Flash UI player's action queue: 0x1460C0 appends a node
    // {item, owner, next} (12 bytes, operator new) at the tail (a2 != 0) or the head of the owner's player queue (player
    // +0xCD8 head, +0xCDC tail); 0x146158 drains it at the end of each player update (0x14A560): each action runs
    // (0x1452F8), its node is deleted (0x1461AC), and head/tail are cleared after the walk (+0xCFC set: dropped
    // undeleted). Logged: every append, and every drain whose queue holds the watched block (with the list). The
    // heap guard's "second delete ... at=0x1461ac" is a node still in a queue after a drain deleted it (docs/dev/GAMEPLAY-HOOKS.md).
    void gowFlashQueueDrain(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t player = GPR_U32(ctx, 4), watch = flashqWatch();
        std::string list;
        bool has = false;
        for (uint32_t node = readGuest32(rdram, player + 0xCD8u), n = 0; node && n < 64; node = readGuest32(rdram, node + 8u), ++n)
        {
            char text[16];
            std::snprintf(text, sizeof(text), " %x", node);
            list += text;
            has |= node == watch;
        }
        if (has)
            std::fprintf(stderr, "[gow-flashq] drain player 0x%x ra=0x%x tail=0x%x list:%s\n", player, GPR_U32(ctx, 31),
                         readGuest32(rdram, player + 0xCDCu), list.c_str());
        sub_00146158_0x146158(rdram, ctx, runtime);
    }

    void gowFlashQueueAppend(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        std::fprintf(stderr, "[gow-flashq] append item=0x%x owner=0x%x tail=%u ra=0x%x\n", GPR_U32(ctx, 4), GPR_U32(ctx, 5),
                     GPR_U32(ctx, 6), GPR_U32(ctx, 31));
        sub_001460C0_0x1460c0(rdram, ctx, runtime);
    }

    // 0x147520(a0 track header: channel count u16 at +2, a2 channel entries {node, key} of 8 bytes)
    void gowGuardTimelineChannels(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t header = GPR_U32(ctx, 4), entries = GPR_U32(ctx, 6);
        if (header != 0u && header < 0x02000000u && entries != 0u && entries < 0x02000000u)
        {
            const uint32_t count = readGuest32(rdram, header) >> 16;
            for (uint32_t i = 0; i < count; ++i)
            {
                const uint32_t node = readGuest32(rdram, entries + i * 8u);
                if (node != 0u && !heap_guard::deletable(rdram, runtime, node))
                {
                    heap_guard::report("timeline channel holds a deleted node", node, GPR_U32(ctx, 31));
                    writeGuest32(rdram, entries + i * 8u, 0u);
                }
            }
        }
        sub_00147520_0x147520(rdram, ctx, runtime);
    }

    // 0x23B6A0(a0 node): child entries {node, key} at *(node+0x28), count u16 at *(node+0x1C)+2
    void gowGuardNodeDestructor(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t node = GPR_U32(ctx, 4);
        const uint32_t definition = node < 0x02000000u ? readGuest32(rdram, node + 0x1Cu) : 0u;
        const uint32_t entries = node < 0x02000000u ? readGuest32(rdram, node + 0x28u) : 0u;
        if (definition != 0u && definition < 0x02000000u && entries != 0u && entries < 0x02000000u)
        {
            const uint32_t count = readGuest32(rdram, definition) >> 16;
            for (uint32_t i = 0; i < count; ++i)
            {
                const uint32_t child = readGuest32(rdram, entries + i * 8u);
                if (child != 0u && !heap_guard::deletable(rdram, runtime, child))
                {
                    heap_guard::report("node destructor: deleted child dropped", child, node);
                    writeGuest32(rdram, entries + i * 8u, 0u);
                }
            }
        }
        sub_0023B6A0_0x23b6a0(rdram, ctx, runtime);
    }

    // 0x149340(a0 list: first index +0x18, count +0x1C, entries {node, key} at +0x20); it has no NULL check, so
    // missing entries are removed and the rest moved up.
    void gowGuardCloneList(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t list = GPR_U32(ctx, 4);
        const uint32_t first = list < 0x02000000u ? readGuest32(rdram, list + 0x18u) : 0u;
        const int32_t count = list < 0x02000000u ? static_cast<int32_t>(readGuest32(rdram, list + 0x1Cu)) : 0;
        const uint32_t entries = list < 0x02000000u ? readGuest32(rdram, list + 0x20u) : 0u;
        // Issue #20 froze in the first pass of the loop on a NULL node although nothing was dropped here: the header
        // itself was out of the range checked below (the loop reads the count again in every pass and would walk
        // whatever follows). Such a list is emptied instead; an entries pointer that is not a live block is not deleted.
        if (ctx->pc == 0x00149340u && list != 0u && list < 0x02000000u &&
            (count >= 0x10000 || (count > 0 && (entries == 0u || entries >= 0x02000000u))))
        {
            heap_guard::report("clone list: bad header, treated as empty", list, GPR_U32(ctx, 31));
            std::fprintf(stderr, "[gow-heapguard] clone list header first=0x%x count=0x%x entries=0x%x\n", first,
                         static_cast<uint32_t>(count), entries);
            writeGuest32(rdram, list + 0x1Cu, 0u);
            const auto it = heap_guard::blocks.find(entries);
            if (it == heap_guard::blocks.end() || !it->second.live)
                writeGuest32(rdram, list + 0x20u, 0u);
        }
        else if (count > 0 && count < 0x10000 && entries != 0u && entries < 0x02000000u)
        {
            uint32_t kept = 0;
            for (uint32_t i = 0; i < static_cast<uint32_t>(count); ++i)
            {
                const uint32_t at = entries + (first + i) * 8u;
                const uint32_t node = readGuest32(rdram, at);
                if (!heap_guard::deletable(rdram, runtime, node))
                {
                    heap_guard::report("clone list: deleted or missing node dropped", node, list);
                    continue;
                }
                const uint32_t key = readGuest32(rdram, at + 4u), to = entries + (first + kept) * 8u;
                writeGuest32(rdram, to, node);
                writeGuest32(rdram, to + 4u, key);
                ++kept;
            }
            writeGuest32(rdram, list + 0x1Cu, kept);
        }
        sub_00149340_0x149340(rdram, ctx, runtime);
    }

    // 0x1D6CD0(a0 player controller 0x29CD58, whose first field is Kratos): called every frame by 0x21FDA0 in the
    // playing game states when the controller exists. It copies the magic and blade levels of the UI variables into
    // Kratos's state and then makes a virtual call on Kratos (vtable +0x6C) with no NULL check. In issue #37 Kratos
    // was still NULL in the first frames after the movie before Pandora's Temple: the vtable was read from address 4
    // and the call went to 0x308B527B, which froze the game. Without Kratos the frame's update is skipped.
    void gowGuardPlayerUpdate(uint8_t *rdram, R5900Context *ctx, PS2Runtime *runtime)
    {
        const uint32_t controller = GPR_U32(ctx, 4);
        if (ctx->pc == 0x001D6CD0u && controller != 0u && controller < 0x02000000u && readGuest32(rdram, controller) == 0u)
        {
            static unsigned reports = 0;
            if (reports < 5u)
            {
                ++reports;
                std::fprintf(stderr, "[gow-heapguard] player update skipped: the controller 0x%x has no Kratos yet\n", controller);
            }
            ctx->pc = GPR_U32(ctx, 31);
            return;
        }
        sub_001D6CD0_0x1d6cd0(rdram, ctx, runtime);
    }
}
