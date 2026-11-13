#pragma once

// GOW-Port: the launcher's "Image quality" settings that the menus of the game change while it runs (lib/gow_integration.cpp).
// Index 0: render resolution, 0..7 = 1x..8x (the internal GS scale); 1: upscaling filter, 0 bicubic (sharp), 1 bilinear, 2 pixel;
// 2: sharpness, 0..100 (percent of the bicubic filter's maximum); 3: FXAA (0 / 1).
int gowQualityCount();
int gowQualityGet(int index);
void gowQualitySet(int index, int value);
