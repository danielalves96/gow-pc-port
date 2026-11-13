// GOW-Port: short notices drawn over the game by the host window (a save backup was made, a slot was loaded...). Posted
// from any thread (the EE thread, the launcher bridge); drawn from the window thread by gowFrameDraw. Not part of the
// game's own UI: they never take input and never touch the guest.
#pragma once

#include <string>

// Shows `title` and `text` (UTF-8) for `seconds`. A newer notice replaces the one on screen. `warning` uses the red accent.
void gowNoticePost(const std::string &title, const std::string &text, double seconds = 6.0, bool warning = false);

// Window thread only: draws the notice, when there is one.
void gowNoticeDraw();

// PC_LANGUAGE is "pt-BR": the host texts follow the launcher's language.
bool gowNoticePortuguese();
