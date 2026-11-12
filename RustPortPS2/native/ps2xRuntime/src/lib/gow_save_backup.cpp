// GOW-Port: dated copies of the memory card, see gow_save_backup.h.
#include "gow_save_backup.h"

#include "ps2_runtime.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <vector>

namespace fs = std::filesystem;

namespace
{
    // 528-byte pages, 16384 of them: the PCSX2 .ps2 layout (ps2xIOP memory_card.h ImageSize)
    constexpr std::uintmax_t kCardImageSize = 528u * 16384u;

    std::mutex &backupMutex()
    {
        static std::mutex mutex;
        return mutex;
    }

    std::tm localTime(std::time_t value)
    {
        std::tm result{};
#if defined(_WIN32)
        localtime_s(&result, &value);
#else
        localtime_r(&value, &result);
#endif
        return result;
    }

    std::string jsonEscape(const std::string &text)
    {
        std::string out;
        for (const char c : text)
        {
            if (c == '\\' || c == '"')
                out += '\\';
            if (static_cast<unsigned char>(c) < 0x20u)
                continue;
            out += c;
        }
        return out;
    }

    fs::path backupsRoot(const fs::path &card)
    {
        if (const char *custom = std::getenv("GOW_SAVE_BACKUPS_DIR"); custom && custom[0])
            return fs::path(custom);
        return card.parent_path() / "save-backups";
    }

    // "20261112-103122" or "20261112-103122-2"
    bool isBackupFolderName(const std::string &name)
    {
        if (name.size() < 15u)
            return false;
        for (size_t i = 0; i < 15u; ++i)
        {
            const bool digit = name[i] >= '0' && name[i] <= '9';
            if (i == 8u ? name[i] != '-' : !digit)
                return false;
        }
        return name.size() == 15u || name[15] == '-';
    }

    bool isAutomatic(const fs::path &folder)
    {
        std::ifstream meta(folder / "backup.json");
        if (!meta)
            return false;
        const std::string text((std::istreambuf_iterator<char>(meta)), std::istreambuf_iterator<char>());
        return text.find("\"source\": \"auto\"") != std::string::npos;
    }

    void pruneAutomatic(const fs::path &root, size_t keep)
    {
        if (keep == 0u)
            return;
        std::error_code error;
        std::vector<fs::path> automatic;
        for (const fs::directory_entry &entry : fs::directory_iterator(root, error))
            if (entry.is_directory(error) && isBackupFolderName(entry.path().filename().string()) && isAutomatic(entry.path()))
                automatic.push_back(entry.path());
        if (automatic.size() <= keep)
            return;
        std::sort(automatic.begin(), automatic.end()); // the name is the time: oldest first
        for (size_t i = 0; i + keep < automatic.size(); ++i)
        {
            fs::remove_all(automatic[i], error);
            std::fprintf(stderr, "[gow-save] old backup removed: %s\n", automatic[i].filename().string().c_str());
        }
    }
}

std::string gowSaveCardPath()
{
    if (const char *setting = std::getenv("GOW_MC0"))
        return setting[0] == '0' && setting[1] == '\0' ? std::string() : std::string(setting);
    const fs::path directory = PS2Runtime::getIoPaths().elfDirectory;
    return directory.empty() ? std::string("Mcd001.ps2") : (directory / "Mcd001.ps2").string();
}

GowSaveBackupResult gowSaveBackup(const char *source)
{
    std::lock_guard<std::mutex> lock(backupMutex());
    GowSaveBackupResult result;
    const std::string cardPath = gowSaveCardPath();
    if (cardPath.empty())
    {
        result.error = "no memory card in slot 1";
        return result;
    }
    std::error_code error;
    const std::uintmax_t size = fs::file_size(cardPath, error);
    if (error || size != kCardImageSize)
    {
        result.error = error ? "the memory card file cannot be read" : "the memory card file has an unexpected size";
        return result;
    }

    const fs::path root = backupsRoot(cardPath);
    fs::create_directories(root, error);
    if (error)
    {
        result.error = "cannot create " + root.string();
        return result;
    }

    // a folder of its own, reserved atomically: two saves in the same second get -2, -3...
    const std::time_t now = std::time(nullptr);
    const std::tm local = localTime(now);
    char stamp[32];
    std::strftime(stamp, sizeof(stamp), "%Y%m%d-%H%M%S", &local);
    fs::path folder;
    for (int attempt = 1; attempt < 100; ++attempt)
    {
        folder = root / (attempt == 1 ? std::string(stamp) : std::string(stamp) + "-" + std::to_string(attempt));
        error.clear();
        if (fs::create_directory(folder, error))
            break;
        folder.clear();
        if (error)
            break;
    }
    if (folder.empty())
    {
        result.error = "cannot reserve a backup folder in " + root.string();
        return result;
    }

    const auto fail = [&](const std::string &message) {
        std::error_code ignored;
        fs::remove_all(folder, ignored); // only the folder this call reserved
        result.error = message;
        return result;
    };

    const fs::path temporary = folder / "Mcd001.ps2.tmp";
    {
        std::ifstream in(cardPath, std::ios::binary);
        std::ofstream out(temporary, std::ios::binary | std::ios::trunc);
        if (!in || !out)
            return fail("cannot open the card or the copy");
        out << in.rdbuf();
        out.close();
        if (!in || !out)
            return fail("the copy was cut short");
    }
    if (fs::file_size(temporary, error) != size || error)
        return fail("the copy has the wrong size");

    {
        char created[40];
        std::strftime(created, sizeof(created), "%Y-%m-%dT%H:%M:%S", &local);
        std::ofstream meta(folder / "backup.json", std::ios::trunc);
        meta << "{\n  \"created\": \"" << created << "\",\n  \"created_unix\": " << static_cast<long long>(now)
             << ",\n  \"source\": \"" << jsonEscape(source ? source : "manual") << "\",\n  \"size\": " << size
             << ",\n  \"card\": \"" << jsonEscape(fs::path(cardPath).filename().string()) << "\"\n}\n";
        if (!meta)
            return fail("cannot write backup.json");
    }
    fs::rename(temporary, folder / "Mcd001.ps2", error);
    if (error)
        return fail("cannot publish the copy");

    result.ok = true;
    result.directory = folder.string();
    pruneAutomatic(root, [] {
        const char *keep = std::getenv("GOW_SAVE_BACKUP_KEEP");
        return keep ? static_cast<size_t>(std::max(0l, std::atol(keep))) : static_cast<size_t>(30);
    }());
    return result;
}

// ---- Loading a slot from outside the game's own menus (gow_save_backup.h) ----------------------------------------------

namespace
{
    std::atomic<int> g_memoryRequest{-1};

    std::mutex &slotsMutex()
    {
        static std::mutex mutex;
        return mutex;
    }

    std::vector<GowSaveSlotInfo> &publishedSlots()
    {
        static std::vector<GowSaveSlotInfo> slots;
        return slots;
    }

    // A request older than this is a leftover (the launcher wrote it and the game never ran): it is dropped.
    constexpr long long kRequestLifetimeSeconds = 600;
}

std::string gowSaveLoadRequestPath()
{
    const std::string card = gowSaveCardPath();
    return card.empty() ? std::string() : (fs::path(card).parent_path() / "load-request").string();
}

void gowSaveLoadRequest(int slot)
{
    if (slot >= 0 && slot < 4)
        g_memoryRequest.store(slot, std::memory_order_release);
}

int gowSaveLoadTakeRequest()
{
    const int memory = g_memoryRequest.exchange(-1, std::memory_order_acq_rel);
    if (memory >= 0)
        return memory;
    static auto lastPoll = std::chrono::steady_clock::time_point{};
    const auto now = std::chrono::steady_clock::now();
    if (now - lastPoll < std::chrono::milliseconds(500))
        return -1;
    lastPoll = now;
    static const std::string path = gowSaveLoadRequestPath();
    std::error_code error;
    if (path.empty() || !fs::exists(path, error))
        return -1;
    // take it by renaming it first, so a request the launcher writes meanwhile is a new file and is never lost half-read
    const std::string taken = path + ".taken";
    fs::remove(taken, error);
    fs::rename(path, taken, error);
    if (error)
        return -1;
    std::ifstream in(taken);
    int slot = -1;
    long long created = 0;
    std::string line;
    while (std::getline(in, line))
    {
        if (line.rfind("slot=", 0) == 0)
            slot = std::atoi(line.c_str() + 5);
        else if (line.rfind("created_unix=", 0) == 0)
            created = std::atoll(line.c_str() + 13);
    }
    in.close();
    fs::remove(taken, error);
    const long long age = static_cast<long long>(std::time(nullptr)) - created;
    if (slot < 0 || slot > 3 || created <= 0 || age > kRequestLifetimeSeconds || age < -60)
    {
        std::fprintf(stderr, "[gow-load] request file ignored (slot=%d, age=%lld s)\n", slot, age);
        return -1;
    }
    return slot;
}

void gowSaveSlotsPublish(std::vector<GowSaveSlotInfo> slots)
{
    std::lock_guard<std::mutex> lock(slotsMutex());
    publishedSlots() = std::move(slots);
}

std::vector<GowSaveSlotInfo> gowSaveSlotsSnapshot()
{
    std::lock_guard<std::mutex> lock(slotsMutex());
    return publishedSlots();
}

std::string gowSavePlayTime(float seconds)
{
    const long total = seconds > 0.0f && seconds < 3.6e7f ? static_cast<long>(seconds) : 0L;
    char text[32];
    std::snprintf(text, sizeof(text), "%ld:%02ld:%02ld", total / 3600, total / 60 % 60, total % 60);
    return text;
}
