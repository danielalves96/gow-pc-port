// GOW-Port: narrow public interface of the in-game performance overlay (gow_osd.inl, drawn from gow_integration.cpp), for code
// that is not the launcher's environment: the pause menu's "In Game Overlay" page (src/game/gow_menu_diag.cpp).
// The launcher decides the initial state (PC_SHOW_FPS, PC_SHOW_GPU, PC_SHOW_CPU, PC_SHOW_RAM); these functions change it
// while the game runs, for the current session only (nothing is written back to the launcher's settings).
#pragma once

// Number of switchable parts, in this order: 0 FPS counter, 1 GPU (usage, temperature, video memory), 2 CPU (usage, clock),
// 3 RAM (the game's own working set).
int gowOverlayCount();

// Whether part `index` is shown (false when out of range).
bool gowOverlayGet(int index);

// Shows or hides part `index`. Ignored when out of range. Safe from any thread.
void gowOverlaySet(int index, bool shown);
