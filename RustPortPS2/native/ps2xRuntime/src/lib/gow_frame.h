#pragma once

// GOW-Port: the "Frame Rate" settings of the launcher that the menus of the game change while it runs (lib/gow_integration.cpp).
// Index 0: FPS limit, 0 = 60, 1 = 120, 2 = 240 (the game logic rate, PC_LOGIC_FPS); 1: VSync (0 / 1); 2: Early FINISH (0 / 1).
int gowFrameCount();
int gowFrameGet(int index);
// false when this session cannot switch it (FPS limit: the 30 FPS mode; Early FINISH: no GS worker)
bool gowFrameAvailable(int index);
void gowFrameSet(int index, int value);
