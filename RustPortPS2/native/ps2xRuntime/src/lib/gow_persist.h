#pragma once

// GOW-Port: a switch the launcher keeps (a cheat, an overlay line, an image adjustment) was changed inside the game: the new state is
// written to the file named by GOW_INGAME_STATE shortly after (gow_integration.cpp), and the launcher reads it when the game ends. Does
// nothing when the variable is not set (a game started by hand).
void gowPersistTouch();
