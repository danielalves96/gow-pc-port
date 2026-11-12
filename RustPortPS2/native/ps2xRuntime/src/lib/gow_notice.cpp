// GOW-Port: short notices over the game (see gow_notice.h). Same look as the RetroAchievements toast, placed at the bottom
// right so the two never overlap.
#include "gow_notice.h"

#include <raylib.h>

#include <algorithm>
#include <chrono>
#include <cstdlib>
#include <cstring>
#include <mutex>

namespace
{
    struct Notice
    {
        std::string title, text;
        bool warning = false;
        std::chrono::steady_clock::time_point until{};
    };

    std::mutex &noticeMutex()
    {
        static std::mutex mutex;
        return mutex;
    }

    Notice &notice()
    {
        static Notice value;
        return value;
    }
}

bool gowNoticePortuguese()
{
    const char *language = std::getenv("PC_LANGUAGE");
    return language && (std::strcmp(language, "pt-BR") == 0 || std::strcmp(language, "pt") == 0);
}

void gowNoticePost(const std::string &title, const std::string &text, double seconds, bool warning)
{
    std::lock_guard<std::mutex> lock(noticeMutex());
    Notice &current = notice();
    current.title = title;
    current.text = text;
    current.warning = warning;
    current.until = std::chrono::steady_clock::now() +
                    std::chrono::duration_cast<std::chrono::steady_clock::duration>(std::chrono::duration<double>(seconds));
}

void gowNoticeDraw()
{
    std::string title, text;
    bool warning = false;
    {
        std::lock_guard<std::mutex> lock(noticeMutex());
        const Notice &current = notice();
        if (current.until <= std::chrono::steady_clock::now())
            return;
        title = current.title;
        text = current.text;
        warning = current.warning;
    }
    const float width = std::min(520.0f, static_cast<float>(GetScreenWidth()) - 32.0f);
    const float height = 68.0f;
    const float x = static_cast<float>(GetScreenWidth()) - width - 18.0f;
    const float y = static_cast<float>(GetScreenHeight()) - height - 22.0f;
    const Color accent = warning ? Color{214, 92, 80, 255} : Color{201, 167, 101, 255};
    DrawRectangleRounded(Rectangle{x, y, width, height}, 0.12f, 8, Color{12, 13, 15, 236});
    DrawRectangleRoundedLinesEx(Rectangle{x, y, width, height}, 0.12f, 8, 1.0f, Color{accent.r, accent.g, accent.b, 230});
    DrawRectangle(static_cast<int>(x), static_cast<int>(y), 4, static_cast<int>(height), accent);
    DrawText(title.c_str(), static_cast<int>(x + 16), static_cast<int>(y + 12), 15, Color{234, 213, 165, 255});
    DrawText(text.c_str(), static_cast<int>(x + 16), static_cast<int>(y + 37), 16, Color{240, 230, 218, 255});
}
