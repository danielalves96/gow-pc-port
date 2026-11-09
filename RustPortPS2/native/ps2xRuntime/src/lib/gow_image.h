// GOW-Port: narrow public interface of the Shade Boost (brightness, contrast, gamma, saturation applied to the final picture in
// gs/gs_host_present.cpp), for the pause menu's "Image Adjustments" page (src/game/gow_menu_diag.cpp).
// The launcher decides the start values (PC_BRIGHTNESS, PC_CONTRAST, PC_GAMMA, PC_SATURATION, 0..100, 50 = no change); these
// functions change them while the game runs, for the current session only (nothing is written back to the launcher's settings).
#pragma once

// Number of settings, in this order: 0 brightness, 1 contrast, 2 gamma, 3 saturation.
int gowImageCount();

// Current value of setting `index`, 0..100 (50 = no change; 50 when out of range).
int gowImageGet(int index);

// Sets setting `index`, clamped to 0..100. Ignored when out of range. Safe from any thread; the next presented frame uses it.
void gowImageSet(int index, int percent);
