import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import { buildHealth, shortenStep, stepLabel } from './components/Shell';
import { availableUpdate, createMockApi, demoActivity, idleActivity, setApi, type MockApi } from './lib/backend';
import type { UpdateState } from './lib/types';
import { readBindings } from './lib/keys';

let api: MockApi;

function boot(ready: boolean, update?: UpdateState) {
  api = createMockApi({ ready, update });
  setApi(api);
  return render(<App />);
}

afterEach(cleanup);

describe('main window', () => {
  it('offers to play when the game is ready, and starts it with the saved settings flushed first', async () => {
    boot(true);
    const user = userEvent.setup();
    const patch = vi.spyOn(api, 'patchSettings');
    const start = vi.spyOn(api, 'startGame');
    await user.click(await screen.findByRole('switch', { name: 'Anti-aliasing (FXAA)' }));
    await user.click(await screen.findByRole('button', { name: 'PLAY' }));
    await waitFor(() => expect(start).toHaveBeenCalled());
    // The change was sent before the game started, not left in the debounce window.
    expect(patch).toHaveBeenCalledWith({ fxaa: false });
    expect(patch.mock.invocationCallOrder[0]).toBeLessThan(start.mock.invocationCallOrder[0]);
  });

  it('shows the running state', async () => {
    boot(true);
    await userEvent.setup().click(await screen.findByRole('button', { name: 'PLAY' }));
    expect(await screen.findByRole('button', { name: 'RUNNING' })).toBeDisabled();
    // The mock game closes by itself after 2.5 s: the button comes back and the toast reports the session.
    expect(await screen.findByText('Game closed. Session: 2 min', {}, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'PLAY' })).toBeEnabled();
  });

  it('keeps the optimization options but not its progress card', async () => {
    boot(true);
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Performance' }));
    // The optimizer is configured and 80 % done in the mock: the player still gets the switches and the button...
    expect(await screen.findByRole('switch', { name: 'Background optimization' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Optimize' })).toBeInTheDocument();
    // ...and no percentage or bar: that is the optimizer's business, not theirs.
    expect(screen.queryByText(/Optimization: \d+ %/)).not.toBeInTheDocument();
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument();
    expect(screen.queryByText(/compiling in the background/)).not.toBeInTheDocument();
  });

  it('switches sections and the language', async () => {
    boot(true);
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Other' }));
    expect(screen.getByRole('heading', { name: 'Other' })).toBeInTheDocument();
    await user.click(screen.getByRole('combobox', { name: 'Language' }));
    await user.click(screen.getByRole('option', { name: 'Português (Brasil)' }));
    expect(await screen.findByRole('heading', { name: 'Outros' })).toBeInTheDocument();
  });
});

describe('game failures', () => {
  it('shows the cause of a recognized failure above the log excerpt, and nothing extra for an unknown one', async () => {
    boot(true);
    const user = userEvent.setup();
    await screen.findByRole('button', { name: 'PLAY' });
    act(() => api.emit('game-finished', {
      code: 1, failed: true, seconds: 3, summary: '[gs-hw] GL unavailable: OpenGL 4.6 core context unavailable',
      failureKind: 'failureOpenGl',
    }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText(/Update the graphics driver/)).toBeInTheDocument();
    expect(within(dialog).getByText(/core context unavailable/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'OK' }));
    act(() => api.emit('game-finished', { code: 3, failed: true, seconds: 3, summary: 'boom', failureKind: null }));
    const next = await screen.findByRole('dialog');
    expect(within(next).getByText('boom')).toBeInTheDocument();
    expect(within(next).queryByText(/Update the graphics driver/)).not.toBeInTheDocument();
  });
});

// GOW-Port: RetroAchievements only with the original USA disc.
describe('achievements with another edition', () => {
  it('replaces the tab with a message when the installed game is not the original disc', async () => {
    api = createMockApi({ ready: true, otherEdition: true });
    setApi(api);
    render(<App />);
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Achievements' }));
    expect(await screen.findByText(/Achievements only work with the original God of War \(USA\) disc/)).toBeInTheDocument();
    expect(screen.queryByRole('switch', { name: 'Enable RetroAchievements' })).not.toBeInTheDocument();
  });

  it('keeps the settings with the original disc', async () => {
    api = createMockApi({ ready: true });
    setApi(api);
    render(<App />);
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Achievements' }));
    expect(await screen.findByRole('switch', { name: 'Enable RetroAchievements' })).toBeInTheDocument();
  });
});

describe('speed profile offer', () => {
  it('offers the optimization on the first screen, starts it and hides it on "Later"', async () => {
    api = createMockApi({ ready: true });
    setApi(api);
    window.localStorage.removeItem('gow.profileOfferDismissed');
    render(<App />);
    const user = userEvent.setup();
    const start = vi.spyOn(api, 'profileStart');
    expect(await screen.findByText('Extra optimization available')).toBeInTheDocument();
    const base = {
      configured: true, compiling: false, captured: 1, compiled: 1, completionRatio: 1, buildProgress: -1, status: 'idle' as const,
      profiling: false, profileReady: false, profileSupported: true, profileStage: 'none' as const, instrumented: false, activity: idleActivity,
    };
    act(() => api.emit('optimizer', { ...base, compiling: true, status: 'compiling', profiling: true, profileStage: 'building', buildProgress: 0.3 }));
    expect(await screen.findByText('Optimizing the game')).toBeInTheDocument();
    act(() => api.emit('optimizer', { ...base, profiling: false }));
    // One click only opens the explanation; starting takes a second, deliberate one.
    await user.click(await screen.findByRole('button', { name: 'Optimize now' }));
    expect(start).not.toHaveBeenCalled();
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText(/Do not touch the keyboard, mouse or controller/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Start optimization' }));
    expect(start).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByText('Extra optimization available')).not.toBeInTheDocument());
    act(() => api.emit('optimizer', { ...base, profiling: false }));
    await user.click(await screen.findByRole('button', { name: 'Later' }));
    expect(screen.queryByText('Extra optimization available')).not.toBeInTheDocument();
    expect(window.localStorage.getItem('gow.profileOfferDismissed')).toBe('1');
    window.localStorage.removeItem('gow.profileOfferDismissed');
  });

  it('"Not now" in the explanation starts nothing', async () => {
    api = createMockApi({ ready: true });
    setApi(api);
    window.localStorage.removeItem('gow.profileOfferDismissed');
    render(<App />);
    const user = userEvent.setup();
    const start = vi.spyOn(api, 'profileStart');
    await user.click(await screen.findByRole('button', { name: 'Optimize now' }));
    await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Not now' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(start).not.toHaveBeenCalled();
  });
});

describe('while "Optimize now" runs', () => {
  it('locks PLAY behind the overlay and warns not to touch anything while the game is open by itself', async () => {
    api = createMockApi({ ready: true, profile: 'training' });
    setApi(api);
    render(<App />);
    expect(await screen.findByRole('heading', { name: 'Optimization in progress' })).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('The game opened by itself: hands off');
    expect(screen.getByRole('button', { name: 'OPTIMIZING' })).toBeDisabled();
    // The current step is marked; the build steps show progress instead of the warning.
    expect(screen.getByText('2. The game opens by itself for about 4 minutes').closest('li')).toHaveAttribute('aria-current', 'step');
  });

  it('shows the progress of the build steps and cancels only after confirming', async () => {
    api = createMockApi({ ready: true, profile: 'finishing' });
    setApi(api);
    render(<App />);
    const user = userEvent.setup();
    const cancel = vi.spyOn(api, 'profileCancel');
    const overlay = await screen.findByRole('dialog');
    expect(within(overlay).queryByRole('alert')).not.toBeInTheDocument();
    expect(within(overlay).getByRole('progressbar', { name: 'Optimization progress' })).toBeInTheDocument();
    await user.click(within(overlay).getByRole('button', { name: 'Cancel optimization' }));
    expect(cancel).not.toHaveBeenCalled();
    await user.click(within(overlay).getByRole('button', { name: 'Keep optimizing' }));
    await user.click(within(overlay).getByRole('button', { name: 'Cancel optimization' }));
    await user.click(within(overlay).getByRole('button', { name: 'Yes, cancel' }));
    expect(cancel).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByRole('heading', { name: 'Optimization in progress' })).not.toBeInTheDocument());
    expect(screen.getByRole('button', { name: 'PLAY' })).toBeEnabled();
  });

  it('asks before the window closes halfway', async () => {
    api = createMockApi({ ready: true, profile: 'building' });
    setApi(api);
    render(<App />);
    await screen.findByRole('heading', { name: 'Optimization in progress' });
    act(() => api.emit('close-blocked', undefined));
    expect(await screen.findByRole('heading', { name: 'The optimization is still running' })).toBeInTheDocument();
  });
});

describe('build details (issue #32)', () => {
  const bootBuilding = (build: 'working' | 'quiet' | 'stuck') => {
    api = createMockApi({ ready: true, rebuildNeeded: true, build });
    setApi(api);
    return render(<App />);
  };

  it('says the build is working, what it does and what it printed last', async () => {
    bootBuilding('working');
    const user = userEvent.setup();
    const openLog = vi.spyOn(api, 'openOptimizerLog');
    await user.click((await screen.findAllByRole('button', { name: 'Details' }))[0]);
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Working: the build is using 9.4 processor cores right now.')).toBeInTheDocument();
    expect(within(dialog).getByText('512 of 1051')).toBeInTheDocument();
    // The installer's step in the player's words, not its raw line.
    expect(within(dialog).getByText('Compiling the game')).toBeInTheDocument();
    // The file name stays, the build folders go.
    expect(within(dialog).getByText('Building CXX object sub_00116EF0_0x116ef0.cpp.obj')).toBeInTheDocument();
    expect(within(dialog).getByText(/\[512\/1051\] Building CXX object/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Open the full log' }));
    expect(openLog).toHaveBeenCalledTimes(1);
    await user.click(within(dialog).getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('explains a long silent step that still uses the processor', async () => {
    bootBuilding('quiet');
    await userEvent.setup().click((await screen.findAllByRole('button', { name: 'Details' }))[0]);
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Working: the build is using 100% of one processor core right now.')).toBeInTheDocument();
    expect(within(dialog).getByText(/print nothing until they finish/)).toBeInTheDocument();
    expect(within(dialog).getByText('14 min ago')).toBeInTheDocument();
  });

  it('warns when nothing has used the processor or printed for a long time', async () => {
    bootBuilding('stuck');
    await userEvent.setup().click((await screen.findAllByRole('button', { name: 'Details' }))[0]);
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText(/it may be stuck/)).toHaveTextContent('No processor use and no output for 24 min');
    expect(within(dialog).queryByText(/print nothing until they finish/)).not.toBeInTheDocument();
  });

  it('is offered only while something compiles', async () => {
    boot(true);
    await screen.findByRole('button', { name: 'PLAY' });
    expect(screen.queryByRole('button', { name: 'Details' })).not.toBeInTheDocument();
  });

  it('tells working, waiting and stuck apart', () => {
    const now = 100 * 60_000;
    const activity = demoActivity('working', now);
    expect(buildHealth(activity, now)).toBe('working');
    expect(buildHealth({ ...activity, cpuPercent: 1, lastBusyMs: now - 60_000 }, now)).toBe('waiting');
    // Silent but busy is not stuck; idle but printing is not stuck either.
    expect(buildHealth({ ...activity, cpuPercent: 1, lastBusyMs: now - 20 * 60_000, lastOutputMs: now - 1_000 }, now)).toBe('waiting');
    expect(buildHealth({ ...activity, cpuPercent: 1, lastBusyMs: now - 20 * 60_000, lastOutputMs: now - 20 * 60_000 }, now)).toBe('stuck');
    expect(buildHealth({ ...activity, cpuPercent: -1 }, now)).toBe('unknown');
    expect(shortenStep('Generating vu1_generated/program0.cpp, vu1_generated/program1.cpp')).toBe('Generating program0.cpp, program1.cpp');
    const t = (key: string) => `<${key}>`;
    expect(stepLabel('Building the game with the speed profile (the first time takes a while)', t)).toBe('<buildStepProfiled>');
    expect(stepLabel('Building the game (the first time takes a while)', t)).toBe('<buildStepBuild>');
    expect(stepLabel('Optimization: the game opens by itself for 240 s', t)).toBe('<buildStepTraining>');
    expect(stepLabel('Something new the installer prints', t)).toBe('Something new the installer prints');
  });
});

describe('a training build left behind', () => {
  it('locks PLAY, says why and offers the rebuild', async () => {
    api = createMockApi({ ready: true, profile: 'leftover' });
    setApi(api);
    render(<App />);
    const user = userEvent.setup();
    const rebuild = vi.spyOn(api, 'rebuildStart');
    expect(await screen.findByText('Slow test version installed: PLAY locked')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'FINISHING' })).toBeDisabled();
    // The offer of "Optimize now" does not compete with it.
    expect(screen.queryByText('Extra optimization available')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Rebuild now' }));
    expect(rebuild).toHaveBeenCalledTimes(1);
  });

  it('refuses the "--play" shortcut with the explanation', async () => {
    await expect(createMockApi({ ready: true, profile: 'leftover' }).startGame()).rejects.toMatchObject({ key: 'errorInstrumented' });
  });
});

describe('rebuild after an update', () => {
  const bootWithRebuild = (rebuildNeeded: boolean) => {
    api = createMockApi({ ready: true, rebuildNeeded });
    setApi(api);
    return render(<App />);
  };

  it('shows nothing when the build is up to date', async () => {
    bootWithRebuild(false);
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Performance' }));
    expect(await screen.findByRole('switch', { name: 'Background optimization' })).toBeInTheDocument();
    expect(screen.queryByText('Rebuild the game to get the latest fixes')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Rebuild now' })).not.toBeInTheDocument();
    expect(screen.queryByText('Game fixes waiting')).not.toBeInTheDocument();
  });

  it('offers the rebuild on the first screen, shows its progress there and goes away when it finishes', async () => {
    bootWithRebuild(true);
    const user = userEvent.setup();
    const start = vi.spyOn(api, 'rebuildStart');
    // No tab is opened: the notice sits over the banner, next to the Play button.
    expect(await screen.findByText('Game fixes waiting')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Rebuild' }));
    expect(start).toHaveBeenCalledTimes(1);
    const snapshot = { profiling: false, profileReady: false, profileSupported: false, profileStage: 'none' as const, instrumented: false, activity: idleActivity, configured: true, compiling: true, captured: 10, compiled: 10, completionRatio: 1, buildProgress: 0.42, status: 'compiling' as const };
    act(() => api.emit('optimizer', snapshot));
    expect(await screen.findByText('Rebuilding the game: 42 %')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: 'Rebuild progress' })).toHaveAttribute('aria-valuenow', '42');
    expect(screen.queryByRole('button', { name: 'Rebuild' })).not.toBeInTheDocument();
    // Before ninja reports a count the bar has no value, only a label.
    act(() => api.emit('optimizer', { ...snapshot, buildProgress: -1 }));
    expect(await screen.findByText('Rebuilding the game')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: 'Rebuild progress' })).not.toHaveAttribute('aria-valuenow');
    act(() => api.emit('optimizer', { ...snapshot, compiling: false, buildProgress: -1, status: 'idle' }));
    await waitFor(() => expect(screen.queryByText('Game fixes waiting')).not.toBeInTheDocument());
    expect(screen.queryByRole('progressbar', { name: 'Rebuild progress' })).not.toBeInTheDocument();
  });

  it('asks before opening the game while the rebuild runs, and says it will be paused', async () => {
    bootWithRebuild(true);
    const user = userEvent.setup();
    const startGame = vi.spyOn(api, 'startGame');
    await screen.findByRole('button', { name: 'PLAY' });
    act(() => api.emit('optimizer', { profiling: false, profileReady: false, profileSupported: false, profileStage: 'none' as const, instrumented: false, activity: idleActivity, configured: true, compiling: true, captured: 10, compiled: 10, completionRatio: 1, buildProgress: 0.3, status: 'compiling' }));
    await user.click(screen.getByRole('button', { name: 'PLAY' }));
    // With "Also while playing" on (the mock's default) the rebuild keeps running: the dialog says the game can stutter.
    expect(await screen.findByText('The game is being rebuilt')).toBeInTheDocument();
    expect(screen.getByText(/it keeps running while you play/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Play anyway' })).toBeInTheDocument();
    expect(startGame).not.toHaveBeenCalled();
    // Waiting closes the dialog and starts nothing.
    await user.click(screen.getByRole('button', { name: 'Wait' }));
    expect(screen.queryByText('The game is being rebuilt')).not.toBeInTheDocument();
    expect(startGame).not.toHaveBeenCalled();
    // With it off (the real default) opening the game pauses the rebuild, and the dialog says so.
    const changed = await api.patchSettings({ optimizeDuringGame: false });
    act(() => api.emit('settings-changed', changed));
    await user.click(screen.getByRole('button', { name: 'PLAY' }));
    expect(await screen.findByText(/the rebuild is paused so the two do not compete/)).toBeInTheDocument();
    await user.click(await screen.findByRole('button', { name: 'Play and pause the rebuild' }));
    await waitFor(() => expect(startGame).toHaveBeenCalledTimes(1));
    expect(screen.queryByText('The game is being rebuilt')).not.toBeInTheDocument();
  });

  it('warns before deleting a build of 0.2.x and keeps the saves out of it', async () => {
    api = createMockApi({ ready: true, legacyBuild: true });
    setApi(api);
    render(<App />);
    const user = userEvent.setup();
    const start = vi.spyOn(api, 'rebuildStart');
    expect(await screen.findByText('Old build: rebuild for 0.3.0')).toBeInTheDocument();
    // The chip does not start it: it explains first, and "Not now" starts nothing.
    await user.click(screen.getByRole('button', { name: 'Rebuild' }));
    expect(await screen.findByText(/the old build is deleted and the game is compiled again from scratch/)).toBeInTheDocument();
    expect(screen.getAllByText(/Your saves and the memory card \(game\/Mcd001\.ps2\)/).length).toBeGreaterThan(0);
    await user.click(screen.getByRole('button', { name: 'Not now' }));
    expect(start).not.toHaveBeenCalled();
    // The Performance tab says the same, and its button is the confirmation.
    await user.click(screen.getByRole('button', { name: 'Performance' }));
    expect(await screen.findByText('Rebuild the game from scratch for 0.3.0')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Delete and rebuild' }));
    expect(start).toHaveBeenCalledTimes(1);
  });

  it('shows the same progress in the Performance tab', async () => {
    bootWithRebuild(true);
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Performance' }));
    expect(await screen.findByRole('button', { name: 'Rebuild now' })).toBeInTheDocument();
    act(() => api.emit('optimizer', { profiling: false, profileReady: false, profileSupported: false, profileStage: 'none' as const, instrumented: false, activity: idleActivity, configured: true, compiling: true, captured: 10, compiled: 10, completionRatio: 1, buildProgress: 0.25, status: 'compiling' }));
    expect(await screen.findAllByRole('progressbar', { name: 'Rebuild progress' })).toHaveLength(2);
    expect(screen.queryByRole('button', { name: 'Rebuild now' })).not.toBeInTheDocument();
  });

  it('announces it once, offers the rebuild in the Performance tab and clears the note when the rebuild succeeds', async () => {
    bootWithRebuild(true);
    const user = userEvent.setup();
    const start = vi.spyOn(api, 'rebuildStart');
    expect(await screen.findByText(/Open Performance to rebuild the game/)).toBeInTheDocument();
    await user.click(await screen.findByRole('button', { name: 'Performance' }));
    expect(await screen.findByText('Rebuild the game to get the latest fixes')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Rebuild now' }));
    expect(start).toHaveBeenCalledTimes(1);
    // The optimizer reports the rebuild running and then finished: the need is asked again and is gone.
    const snapshot = { profiling: false, profileReady: false, profileSupported: false, profileStage: 'none' as const, instrumented: false, activity: idleActivity, configured: true, compiling: true, captured: 10, compiled: 10, completionRatio: 1, buildProgress: 0.5, status: 'compiling' as const };
    act(() => api.emit('optimizer', snapshot));
    act(() => api.emit('optimizer', { ...snapshot, compiling: false, buildProgress: -1, status: 'idle' }));
    await waitFor(() => expect(screen.queryByText('Rebuild the game to get the latest fixes')).not.toBeInTheDocument());
  });

  it('says so when the rebuild fails, keeps offering it and opens the log', async () => {
    bootWithRebuild(true);
    const user = userEvent.setup();
    const openLog = vi.spyOn(api, 'openOptimizerLog');
    await user.click(await screen.findByRole('button', { name: 'Performance' }));
    expect(await screen.findByRole('button', { name: 'Rebuild now' })).toBeInTheDocument();
    expect(screen.queryByText('The last rebuild of the game failed')).not.toBeInTheDocument();
    const snapshot = { profiling: false, profileReady: false, profileSupported: false, profileStage: 'none' as const, instrumented: false, activity: idleActivity, configured: true, compiling: true, captured: 10, compiled: 10, completionRatio: 1, buildProgress: 0.5, status: 'compiling' as const };
    act(() => api.emit('optimizer', snapshot));
    act(() => api.emit('optimizer', { ...snapshot, compiling: false, buildProgress: -1, status: 'error' }));
    expect(await screen.findByText('The last rebuild of the game failed')).toBeInTheDocument();
    expect(screen.getByText(/The rebuild of the game failed\. Open Performance/)).toBeInTheDocument();
    // The failed build keeps its old marker: the offer to rebuild stays.
    expect(screen.getByRole('button', { name: 'Rebuild now' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Open the rebuild log' }));
    expect(openLog).toHaveBeenCalledTimes(1);
  });

  it('disables the button with the reason while the game runs', async () => {
    bootWithRebuild(true);
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Performance' }));
    expect(await screen.findByRole('button', { name: 'Rebuild now' })).toBeEnabled();
    act(() => api.emit('game-running', true));
    expect(screen.getByRole('button', { name: 'Rebuild now' })).toBeDisabled();
    expect(screen.getByText('Close the game to rebuild.')).toBeInTheDocument();
  });
});

describe('graphics', () => {
  it('turns the widescreen (PCSX2) patch on and off', async () => {
    boot(true);
    const user = userEvent.setup();
    const patch = vi.spyOn(api, 'patchSettings');
    // On by default; off keeps the game's original 4:3 picture (the runtime is not given PC_WIDESCREEN).
    await user.click(await screen.findByRole('switch', { name: 'Widescreen (Hor+)' }));
    await waitFor(() => expect(patch).toHaveBeenCalledWith({ widescreen: false }));
    expect(screen.getByRole('combobox', { name: 'Aspect ratio' })).toBeDisabled();
    // Turning it back on widens the camera at the chosen aspect and moves the window into that aspect.
    await user.click(screen.getByRole('switch', { name: 'Widescreen (Hor+)' }));
    await waitFor(() =>
      expect(patch).toHaveBeenLastCalledWith(expect.objectContaining({ widescreen: true, aspectMode: '16:9', stretch: false })),
    );
    expect(screen.getByRole('combobox', { name: 'Aspect ratio' })).toBeEnabled();
  });

  it('offers 32:9 as very experimental with a short label, and explains the limits in a note once chosen', async () => {
    boot(true);
    const user = userEvent.setup();
    expect(screen.queryByText('32:9 is very experimental')).not.toBeInTheDocument();
    await user.click(await screen.findByRole('combobox', { name: 'Aspect ratio' }));
    const option = await screen.findByRole('option', { name: /32:9 ultrawide/ });
    // a short label (it has to fit the select on one line) with the warning as its second line
    expect(option).toHaveTextContent('very experimental');
    expect(option.textContent).not.toMatch(/stretched/);
    await user.click(option);
    expect(await screen.findByText('32:9 is very experimental')).toBeInTheDocument();
    expect(screen.getByText(/HUD and menus are stretched across the whole width/)).toBeInTheDocument();
  });
});

describe('controls', () => {
  it('captures a physical key into the second slot and saves the map as JSON', async () => {
    boot(true);
    const user = userEvent.setup();
    const patch = vi.spyOn(api, 'patchSettings');
    await user.click(await screen.findByRole('button', { name: 'Controls' }));
    const row = (await screen.findByText('Special power (cast magic)')).parentElement!;
    const slots = within(row).getAllByRole('button');
    await user.click(slots[1]); // second slot of L2 (the first is Shift)
    expect(await screen.findByRole('heading', { name: /press the new key/i })).toBeInTheDocument();
    window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyX', bubbles: true, cancelable: true }));
    await waitFor(() => expect(screen.queryByRole('heading', { name: /press the new key/i })).not.toBeInTheDocument());
    await waitFor(() => expect(patch).toHaveBeenCalled());
    const saved = (patch.mock.calls.at(-1)![0] as { keys: string }).keys;
    expect(readBindings(saved).l2).toEqual([340, 88]);
  });

  it('Escape cancels without changing anything', async () => {
    boot(true);
    const user = userEvent.setup();
    const patch = vi.spyOn(api, 'patchSettings');
    await user.click(await screen.findByRole('button', { name: 'Controls' }));
    const row = (await screen.findByText('Jump')).parentElement!;
    await user.click(within(row).getAllByRole('button')[0]);
    window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Escape', key: 'Escape', bubbles: true, cancelable: true }));
    await waitFor(() => expect(screen.queryByRole('heading', { name: /press the new key/i })).not.toBeInTheDocument());
    expect(patch).not.toHaveBeenCalled();
  });

  it('captures a mouse button', async () => {
    boot(true);
    const user = userEvent.setup();
    const patch = vi.spyOn(api, 'patchSettings');
    await user.click(await screen.findByRole('button', { name: 'Controls' }));
    const row = (await screen.findByText('Jump')).parentElement!;
    await user.click(within(row).getAllByRole('button')[0]);
    window.dispatchEvent(new MouseEvent('mousedown', { button: 1, bubbles: true, cancelable: true }));
    await waitFor(() => expect(patch).toHaveBeenCalled());
    expect(readBindings((patch.mock.calls.at(-1)![0] as { keys: string }).keys).cross).toEqual([1002]);
  });
});

describe('installation wizard', () => {
  beforeEach(() => {
    vi.useRealTimers();
  });

  it('walks from the ISO to a finished installation and adopts the result', async () => {
    boot(false);
    const user = userEvent.setup();
    const install = vi.spyOn(api, 'installStart').mockImplementation(async () => {
      api.emit('install-progress', { stage: 5, stageProgress: 0.5, recentLines: ['[1/2] x'], activity: idleActivity });
      api.emit('install-done', { ok: true, cancelled: false, error: '', elf: 'C:\\g\\game\\SCUS_973.99', executable: 'C:\\g\\build\\ps2xRuntime\\gow-2005.exe', failureKind: null });
    });
    const complete = vi.spyOn(api, 'installComplete');

    // 0: the ISO is searched on the PC and found.
    expect(await screen.findByText('Choose your God of War ISO')).toBeInTheDocument();
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(next());
    // 1: location (free space shown, accent-free path accepted).
    expect(await screen.findByText('Where to install')).toBeInTheDocument();
    await waitFor(() => expect(next()).toBeEnabled());
    expect(screen.getByText(/Free space: 412 GB/)).toBeInTheDocument();
    await user.click(next());
    // 2: components, because the mock lacks Build Tools and Git.
    expect(await screen.findByText('Required components')).toBeInTheDocument();
    await user.click(next());
    // 3: summary.
    expect(await screen.findByText('Ready to install')).toBeInTheDocument();
    expect(screen.getByText(/Visual Studio Build Tools/)).toBeInTheDocument();
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    // 4 -> 5: success.
    expect(await screen.findByText('All set!')).toBeInTheDocument();
    expect(install).toHaveBeenCalledWith({
      iso: 'D:\\Games\\God of War (USA).iso', destination: 'C:\\GodOfWar-PC', installTools: true, vu1MemoryLimitGb: 4,
    });
    await user.click(screen.getByRole('button', { name: 'OK' }));
    await waitFor(() => expect(complete).toHaveBeenCalledWith({
      destination: 'C:\\GodOfWar-PC', elf: 'C:\\g\\game\\SCUS_973.99', executable: 'C:\\g\\build\\ps2xRuntime\\gow-2005.exe',
    }));
    await waitFor(() => expect(screen.queryByText('All set!')).not.toBeInTheDocument());
  });

  it('shows what the installation is doing, and never calls a slow download stuck', async () => {
    boot(false);
    const user = userEvent.setup();
    vi.spyOn(api, 'installStart').mockResolvedValue(undefined);
    const openLog = vi.spyOn(api, 'openInstallLog');
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(next());
    await screen.findByText('Where to install');
    await waitFor(() => expect(next()).toBeEnabled());
    await user.click(next());
    await screen.findByText('Required components');
    await user.click(next());
    await screen.findByText('Ready to install');
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    // Nothing to show before the installer printed anything.
    expect(screen.queryByRole('button', { name: 'Details' })).not.toBeInTheDocument();
    // A download that has printed nothing for a long time, with an idle processor: waiting on the connection, not stuck.
    const now = Date.now();
    const download = { ...demoActivity('download', now), startedAtMs: now - 40 * 60_000, lastOutputMs: now - 30 * 60_000, lastBusyMs: now - 30 * 60_000 };
    act(() => api.emit('install-progress', { stage: 0, stageProgress: -1, recentLines: download.recentLines, activity: download }));
    await user.click(await screen.findByRole('button', { name: 'Details' }));
    expect(await screen.findByText(/Downloading: this step waits on your internet connection/)).toBeInTheDocument();
    expect(screen.getByText('Downloading the compiler (about 170 MB)')).toBeInTheDocument();
    expect(screen.queryByText(/it may be stuck/)).not.toBeInTheDocument();
    // The window follows the installation live, and its log is the installation's.
    act(() => api.emit('install-progress', { stage: 5, stageProgress: 0.5, recentLines: [], activity: demoActivity('working') }));
    expect(await screen.findByText('Working: the build is using 9.4 processor cores right now.')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Open the full log' }));
    expect(openLog).toHaveBeenCalledTimes(1);
  });

  it('applies a translation patch to the original ISO and installs the translated copy', async () => {
    boot(false);
    const user = userEvent.setup();
    const install = vi.spyOn(api, 'installStart').mockResolvedValue(undefined);
    const start = vi.spyOn(api, 'isoPatchStart');
    vi.spyOn(api, 'pick').mockResolvedValue('D:\\Patches\\God of War PT-BR.xdelta');
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    // The original ISO offers the patch; ticking it blocks NEXT until the patch is applied.
    await user.click(screen.getByText('Apply a patch (.xdelta)'));
    expect(next()).toBeDisabled();
    expect(screen.getByText('Apply the patch, or untick the option, to continue.')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Choose patch' }));
    expect(await screen.findByText('Creates God of War PT-BR.iso in the same folder (about 8 GB).')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Apply patch' }));
    expect(start).toHaveBeenCalledWith('D:\\Games\\God of War (USA).iso', 'D:\\Patches\\God of War PT-BR.xdelta');
    expect(await screen.findByRole('progressbar', { name: 'Patch progress' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Stop' })).toBeInTheDocument();
    // Done: the translated copy replaces the ISO, the original is untouched and can be taken back.
    expect(await screen.findByText('Patch applied: God of War PT-BR.iso', {}, { timeout: 4000 })).toBeInTheDocument();
    expect(screen.getByText(/The original ISO was not changed/)).toBeInTheDocument();
    expect(screen.queryByText('Apply a patch (.xdelta)')).not.toBeInTheDocument();
    await waitFor(() => expect(next()).toBeEnabled());
    await user.click(next());
    await user.click(await screen.findByRole('button', { name: 'NEXT' }));
    await user.click(await screen.findByRole('button', { name: 'NEXT' }));
    await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    await waitFor(() => expect(install).toHaveBeenCalledWith(expect.objectContaining({ iso: 'D:\\Games\\God of War PT-BR.iso' })));
  });

  it('converts a CHD to an ISO next to it before going on, with progress and Stop', async () => {
    boot(false);
    const user = userEvent.setup();
    const install = vi.spyOn(api, 'installStart').mockResolvedValue(undefined);
    const start = vi.spyOn(api, 'isoChdStart');
    vi.spyOn(api, 'pick').mockResolvedValue('D:\\Games\\God of War.chd');
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(screen.getByRole('button', { name: 'Browse' }));
    // Validated in place; NEXT waits for the conversion, and the patch option waits for the ISO.
    expect(await screen.findByText(/CHD image: original disc verified inside it/)).toBeInTheDocument();
    expect(await screen.findByText(/Writes God of War.iso \(7.9 GB\) in the CHD’s folder/)).toBeInTheDocument();
    expect(next()).toBeDisabled();
    expect(screen.queryByText('Apply a patch (.xdelta)')).not.toBeInTheDocument();
    // Stopped: nothing changes, and it can be started again.
    await user.click(screen.getByRole('button', { name: 'Convert' }));
    expect(start).toHaveBeenCalledWith('D:\\Games\\God of War.chd');
    expect(await screen.findByRole('progressbar', { name: 'Conversion progress' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Stop' }));
    expect(await screen.findByText('Cancelled. Nothing was created.', {}, { timeout: 4000 })).toBeInTheDocument();
    expect(next()).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Convert' }));
    expect(await screen.findByText('Converted from the CHD: God of War.iso', {}, { timeout: 4000 })).toBeInTheDocument();
    expect(screen.getByText(/Your CHD was not changed/)).toBeInTheDocument();
    expect(screen.getByText('Apply a patch (.xdelta)')).toBeInTheDocument();
    await waitFor(() => expect(next()).toBeEnabled());
    await user.click(next());
    await user.click(await screen.findByRole('button', { name: 'NEXT' }));
    await user.click(await screen.findByRole('button', { name: 'NEXT' }));
    await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    await waitFor(() => expect(install).toHaveBeenCalledWith(expect.objectContaining({ iso: 'D:\\Games\\God of War.iso' })));
    // Two simulated conversions (cancelled, then complete) with 4 s waits: over the 5 s default on a slow CI runner.
  }, 20000);

  it('never offers a patch for a modified CHD, before or after converting it', async () => {
    boot(false);
    const user = userEvent.setup();
    const translated = { ok: true, reason: null, hash: 'ff2f', translation: true };
    vi.spyOn(api, 'validateIso').mockResolvedValue(translated);
    vi.spyOn(api, 'isoChdStart').mockImplementation(async (chd) => {
      setTimeout(() => api.emit('iso-chd-done', {
        ok: true, path: chd.replace(/\.chd$/i, '.iso'), validation: translated, error: null, detail: '',
      }), 50);
    });
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.clear(screen.getByDisplayValue('D:\\Games\\God of War (USA).iso'));
    await user.type(screen.getByPlaceholderText(/./), 'D:\\Games\\God of War PT-BR.chd');
    await user.click(await screen.findByRole('button', { name: 'Convert' }));
    expect(await screen.findByText('Converted from the CHD: God of War PT-BR.iso')).toBeInTheDocument();
    expect(screen.getByText(/Modified disc recognized/)).toBeInTheDocument();
    expect(screen.queryByText('Apply a patch (.xdelta)')).not.toBeInTheDocument();
    await waitFor(() => expect(next()).toBeEnabled());
  });

  it('names a CHD that cannot be read', async () => {
    boot(false);
    const user = userEvent.setup();
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    vi.spyOn(api, 'validateIso').mockResolvedValue({ ok: false, reason: 'chd', hash: '' });
    await user.clear(screen.getByDisplayValue('D:\\Games\\God of War (USA).iso'));
    await user.type(screen.getByPlaceholderText(/./), 'D:\\Games\\broken.chd');
    expect(await screen.findByText(/This CHD cannot be read/)).toBeInTheDocument();
    expect(next()).toBeDisabled();
    expect(screen.queryByRole('button', { name: 'Convert' })).not.toBeInTheDocument();
  });

  it('explains a wrong patch, hides the option for a translated ISO and goes back to the original', async () => {
    boot(false);
    const user = userEvent.setup();
    const pick = vi.spyOn(api, 'pick').mockResolvedValue('D:\\Patches\\readme.txt');
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(screen.getByText('Apply a patch (.xdelta)'));
    await user.click(screen.getByRole('button', { name: 'Choose patch' }));
    expect(await screen.findByText('This file is not an xdelta patch.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Apply patch' })).toBeDisabled();
    // A good patch, applied, then "Use the original" brings the original ISO and the option back.
    pick.mockResolvedValue('D:\\Patches\\PT-BR.xdelta');
    await user.click(screen.getByRole('button', { name: 'Choose patch' }));
    await user.click(await screen.findByRole('button', { name: 'Apply patch' }));
    await user.click(await screen.findByRole('button', { name: 'Use the original' }, { timeout: 4000 }));
    expect(await screen.findByText('Apply a patch (.xdelta)')).toBeInTheDocument();
    expect(screen.getByDisplayValue('D:\\Games\\God of War (USA).iso')).toBeInTheDocument();
    // An ISO that is already a translation never shows the option.
    vi.spyOn(api, 'validateIso').mockResolvedValue({ ok: true, reason: null, hash: 'x', translation: true });
    await user.clear(screen.getByDisplayValue('D:\\Games\\God of War (USA).iso'));
    await user.type(screen.getByPlaceholderText(/./), 'D:\\Games\\God of War PT-BR.iso');
    expect(await screen.findByText(/Modified disc recognized/)).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText('Apply a patch (.xdelta)')).not.toBeInTheDocument());
  });

  it('blocks the first step when the processor lacks SSE4.1, even with the ISO verified', async () => {
    boot(false);
    vi.spyOn(api, 'wizardInfo').mockResolvedValue({
      environment: { vs: true, cmakeUnusable: false, git: true, installer: true, missing: [], missingPackages: [], installCommand: '', cpuMissing: ['SSE4.1'], kitMissing: [] },
      hasManifest: true, defaultDestination: 'C:\\GodOfWar-PC',
    });
    expect(await screen.findByRole('alert')).toHaveTextContent(/does not support SSE4\.1/);
    // The ISO is found and verified, so only the processor keeps NEXT disabled.
    expect(await screen.findByText(/original ISO verified/, {}, { timeout: 4000 })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'NEXT' })).toBeDisabled();
  });

  it('shows no processor warning when nothing is missing', async () => {
    boot(false);
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    expect(screen.queryByText(/does not support SSE4\.1/)).not.toBeInTheDocument();
  });

  it('shows no components when Visual Studio has a usable CMake, and installs nothing', async () => {
    boot(false);
    vi.spyOn(api, 'wizardInfo').mockResolvedValue({
      environment: { vs: true, cmakeUnusable: false, git: true, installer: true, missing: [], missingPackages: [], installCommand: '', cpuMissing: [], kitMissing: [] },
      hasManifest: true, defaultDestination: 'C:\\GodOfWar-PC',
    });
    const user = userEvent.setup();
    const install = vi.spyOn(api, 'installStart').mockResolvedValue(undefined);
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(next());
    await waitFor(() => expect(next()).toBeEnabled());
    await user.click(next());
    // Location goes straight to the summary: there is no components step.
    expect(await screen.findByText('Ready to install')).toBeInTheDocument();
    expect(screen.queryByText('Required components')).not.toBeInTheDocument();
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    await waitFor(() => expect(install).toHaveBeenCalledWith(expect.objectContaining({ installTools: false })));
  });

  it('offers nothing for a Visual Studio without a usable CMake: the installer downloads a portable one', async () => {
    boot(false);
    vi.spyOn(api, 'wizardInfo').mockResolvedValue({
      environment: { vs: true, cmakeUnusable: true, git: true, installer: true, missing: [], missingPackages: [], installCommand: '', cpuMissing: [], kitMissing: [] },
      hasManifest: true, defaultDestination: 'C:\\GodOfWar-PC',
    });
    const user = userEvent.setup();
    const install = vi.spyOn(api, 'installStart').mockResolvedValue(undefined);
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(next());
    await waitFor(() => expect(next()).toBeEnabled());
    await user.click(next());
    // Location goes straight to the summary, as with a usable CMake: portable CMake and Ninja need no component step.
    expect(await screen.findByText('Ready to install')).toBeInTheDocument();
    expect(screen.queryByText('Required components')).not.toBeInTheDocument();
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    await waitFor(() => expect(install).toHaveBeenCalledWith(expect.objectContaining({ installTools: false })));
  });

  it('on Linux lists the missing packages and lets the user continue once a new check finds none', async () => {
    api = createMockApi({ ready: false });
    setApi(api);
    const environment = {
      vs: false, cmakeUnusable: false, git: true, installer: true, missing: ['ninja', 'alsa'], missingPackages: ['ninja-build', 'libasound2-dev'],
      installCommand: 'sudo apt install ninja-build libasound2-dev', cpuMissing: [], kitMissing: [],
    };
    vi.spyOn(api, 'wizardInfo')
      .mockResolvedValueOnce({ environment, hasManifest: true, defaultDestination: '/home/me/gow' })
      .mockResolvedValue({
        environment: { ...environment, vs: true, missing: [], missingPackages: [], installCommand: '', cpuMissing: [], kitMissing: [] },
        hasManifest: true, defaultDestination: '/home/me/gow',
      });
    const user = userEvent.setup();
    render(<App />);
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(next());
    await waitFor(() => expect(next()).toBeEnabled());
    await user.click(next());
    // 2: nothing to tick; the missing packages and the command to run, and no way forward yet.
    expect(await screen.findByText('Missing: ninja, alsa')).toBeInTheDocument();
    expect(screen.getByText('sudo apt install ninja-build libasound2-dev')).toBeInTheDocument();
    expect(screen.queryByText('Visual Studio Build Tools (C++)')).not.toBeInTheDocument();
    expect(next()).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'CHECK AGAIN' }));
    await waitFor(() => expect(screen.queryByText('Missing: ninja, alsa')).not.toBeInTheDocument());
    expect(next()).toBeEnabled();
  });

  it('shows the failure and lets the user try again', async () => {
    boot(false);
    const user = userEvent.setup();
    let attempt = 0;
    vi.spyOn(api, 'installStart').mockImplementation(async () => {
      attempt++;
      api.emit('install-done', attempt === 1
        ? { ok: false, cancelled: false, error: 'cmake not found', elf: '', executable: '', failureKind: null }
        : { ok: true, cancelled: false, error: '', elf: 'e', executable: 'x', failureKind: null });
    });
    await waitFor(() => expect(screen.getByRole('button', { name: 'NEXT' })).toBeEnabled(), { timeout: 4000 });
    for (const label of ['NEXT', 'NEXT', 'NEXT']) await user.click(await screen.findByRole('button', { name: label }));
    await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    expect(await screen.findByText('Could not finish')).toBeInTheDocument();
    expect(screen.getByText('cmake not found')).toBeInTheDocument();
    // An unrecognized failure gets no explanation, only the raw text.
    expect(screen.queryByText(/A download failed/)).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'TRY AGAIN' }));
    expect(await screen.findByText('All set!')).toBeInTheDocument();
  });

  it('explains a recognized failure above the raw text', async () => {
    boot(false);
    const user = userEvent.setup();
    vi.spyOn(api, 'installStart').mockImplementation(async () => {
      api.emit('install-done', {
        ok: false, cancelled: false, elf: '', executable: '', failureKind: 'failureDownload',
        error: 'Could not download https://example.org/x.zip (timed out).',
      });
    });
    await waitFor(() => expect(screen.getByRole('button', { name: 'NEXT' })).toBeEnabled(), { timeout: 4000 });
    for (const label of ['NEXT', 'NEXT', 'NEXT']) await user.click(await screen.findByRole('button', { name: label }));
    await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    expect(await screen.findByText(/A download failed/)).toBeInTheDocument();
    expect(screen.getByText('Could not download https://example.org/x.zip (timed out).')).toBeInTheDocument();
  });

  it('lists the missing kit files and stops at the location step', async () => {
    boot(false);
    vi.spyOn(api, 'wizardInfo').mockResolvedValue({
      environment: {
        vs: true, cmakeUnusable: false, git: true, installer: false, missing: [], missingPackages: [], installCommand: '',
        cpuMissing: [], kitMissing: ['install.ps1', 'bin/ps2-tools.exe'],
      },
      hasManifest: true, defaultDestination: 'C:\GodOfWar-PC',
    });
    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole('button', { name: 'NEXT' })).toBeEnabled(), { timeout: 4000 });
    await user.click(screen.getByRole('button', { name: 'NEXT' }));
    expect(await screen.findByText(/Installation kit files are missing: install\.ps1, bin\/ps2-tools\.exe/)).toBeInTheDocument();
    expect(screen.queryByText(/installer script \(install\.ps1/)).not.toBeInTheDocument();
    await screen.findByText(/Free space/);
    expect(screen.getByRole('button', { name: 'NEXT' })).toBeDisabled();
  });

  it('rejects a destination with accents', async () => {
    boot(false);
    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole('button', { name: 'NEXT' })).toBeEnabled(), { timeout: 4000 });
    await user.click(screen.getByRole('button', { name: 'NEXT' }));
    const field = await screen.findByDisplayValue('C:\\GodOfWar-PC');
    await user.clear(field);
    await user.type(field, 'C:\\Usuários\\x');
    expect(await screen.findByText(/cannot contain accents/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'NEXT' })).toBeDisabled();
  });
});

describe('texture pack installation', () => {
  beforeEach(() => {
    vi.useRealTimers();
  });

  /** The Textures section with custom textures on and the pack card offered; the installation does not start by itself. */
  async function openTheCard() {
    boot(true);
    const install = vi.spyOn(api, 'installTexturePack').mockImplementation(async () => undefined);
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Textures' }));
    await user.click(await screen.findByRole('switch', { name: 'Custom textures' }));
    await user.click(await screen.findByRole('button', { name: 'Download and install' }));
    await waitFor(() => expect(install).toHaveBeenCalled());
    return user;
  }
  const bar = () => Number(screen.getByRole('progressbar', { name: 'Texture pack' }).getAttribute('aria-valuenow'));
  const progress = (stage: 'downloading' | 'extracting', done: number, total: number, overall: number) =>
    act(() => api.emit('texture-pack-progress', { stage, done, total, overall }));

  it('holds the interface still while it installs and frees it when it ends', async () => {
    await openTheCard();
    // Nothing else can be opened, the game cannot start and the options that change the folder are off.
    expect(screen.getByRole('button', { name: 'Graphics' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Performance' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Textures' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'INSTALLING TEXTURES' })).toBeDisabled();
    expect(screen.getByRole('switch', { name: 'Custom textures' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Downloading…' })).toBeDisabled();
    expect(screen.getByText(/keep this tab and the launcher open/)).toBeInTheDocument();

    progress('downloading', 2_000_000_000, 4_000_000_000, 0.4);
    expect(screen.getByText(/Downloading: 1.86 of 3.73 GB · 40 %/)).toBeInTheDocument();
    progress('extracting', 120, 420, 0.9);
    expect(screen.getByText('Extracting: 120 / 420 files · 90 %')).toBeInTheDocument();

    act(() => api.emit('texture-pack-done', {
      ok: true, error: null, code: null,
      status: { installed: true, files: 420, bytes: 5_100_000_000, etag: 'e', uploaded: '' },
    }));
    expect(await screen.findByText(/Texture pack installed: 420 files/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Graphics' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'PLAY' })).toBeEnabled();
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Download and install' })).not.toBeInTheDocument();
  });

  it('never moves the bar backwards, even if a late report arrives', async () => {
    await openTheCard();
    progress('downloading', 3_000_000_000, 4_000_000_000, 0.6);
    expect(bar()).toBe(60);
    progress('downloading', 1_000_000_000, 4_000_000_000, 0.2);
    expect(bar()).toBe(60);
    progress('extracting', 1, 420, 0.81);
    expect(bar()).toBe(81);
  });

  it('frees the interface and says why when the installation fails', async () => {
    await openTheCard();
    progress('downloading', 1_000_000_000, 4_000_000_000, 0.2);
    act(() => api.emit('texture-pack-done', { ok: false, status: null, error: 'could not reach the texture server', code: 'network' }));
    expect(await screen.findByText('The texture pack could not be installed')).toBeInTheDocument();
    expect(screen.getByText('could not reach the texture server')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Graphics' })).toBeEnabled();
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument();
    // The card is still there, to try again.
    expect(screen.getByRole('button', { name: 'Download and install' })).toBeEnabled();
  });

  it('asks before the window closes during the installation', async () => {
    const user = await openTheCard();
    const forceClose = vi.spyOn(api, 'forceClose');
    progress('downloading', 1_000_000_000, 4_000_000_000, 0.2);

    act(() => api.emit('close-blocked', undefined));
    expect(await screen.findByText('The texture pack is still installing')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'KEEP INSTALLING' }));
    expect(screen.queryByText('The texture pack is still installing')).not.toBeInTheDocument();
    expect(forceClose).not.toHaveBeenCalled();

    act(() => api.emit('close-blocked', undefined));
    await user.click(await screen.findByRole('button', { name: 'CLOSE ANYWAY' }));
    expect(forceClose).toHaveBeenCalledTimes(1);
  });

  it('starts only one installation, however many times it is asked', async () => {
    const user = await openTheCard();
    const install = vi.mocked(api.installTexturePack);
    expect(install).toHaveBeenCalledTimes(1);
    // A second click on the disabled button does nothing.
    await user.click(screen.getByRole('button', { name: 'Downloading…' }));
    expect(install).toHaveBeenCalledTimes(1);
  });
});

describe('launcher update notice', () => {
  const notice = () => screen.queryByText(/Version 0\.2\.0 is available/);
  const publish = (state: UpdateState) => act(() => api.emit('update-state', state));

  it('shows nothing when there is no newer release', async () => {
    boot(true);
    await screen.findByRole('button', { name: 'PLAY' });
    expect(notice()).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Update' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Open release page' })).not.toBeInTheDocument();
  });

  it('shows the version and an Update button that stay put when the section changes', async () => {
    boot(true, availableUpdate());
    const user = userEvent.setup();
    expect(await screen.findByText('Version 0.2.0 is available')).toBeInTheDocument();
    const button = screen.getByRole('button', { name: 'Update' });
    expect(button).toBeEnabled();
    const titleBar = button.closest('header');
    for (const section of ['Performance', 'Controls', 'Other']) {
      await user.click(screen.getByRole('button', { name: section }));
      expect(screen.getByRole('button', { name: 'Update' }).closest('header')).toBe(titleBar);
    }
  });

  it('appears when the check finishes after the window opened', async () => {
    boot(true);
    await screen.findByRole('button', { name: 'PLAY' });
    publish(availableUpdate());
    expect(await screen.findByRole('button', { name: 'Update' })).toBeEnabled();
  });

  it('downloads, shows the progress and then the restart', async () => {
    boot(true, availableUpdate());
    const install = vi.spyOn(api, 'updateInstall');
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Update' }));
    expect(install).toHaveBeenCalledTimes(1);
    publish({ ...availableUpdate(), phase: 'downloading', done: 50, total: 200 });
    expect(screen.getByText('Downloading the update: 25 %')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: 'Update progress' })).toHaveAttribute('aria-valuenow', '25');
    expect(screen.queryByRole('button', { name: 'Update' })).not.toBeInTheDocument();
    publish({ ...availableUpdate(), phase: 'installing', done: 200, total: 200 });
    expect(screen.getByText('Installing the update, the launcher will restart')).toBeInTheDocument();
  });

  it('keeps the bar moving without a known size', async () => {
    boot(true);
    await screen.findByRole('button', { name: 'PLAY' });
    publish({ ...availableUpdate(), phase: 'downloading', done: 1000, total: 0 });
    expect(screen.getByText('Downloading the update')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: 'Update progress' })).not.toHaveAttribute('aria-valuenow');
  });

  it('says what went wrong and retries', async () => {
    boot(true);
    await screen.findByRole('button', { name: 'PLAY' });
    publish({ ...availableUpdate(), phase: 'error', error: 'Download request failed with status: 503' });
    expect(screen.getByText('Could not update to 0.2.0')).toBeInTheDocument();
    expect(screen.getByText('Could not update to 0.2.0').parentElement).toHaveAttribute('title', 'Download request failed with status: 503');
    const install = vi.spyOn(api, 'updateInstall');
    await userEvent.setup().click(screen.getByRole('button', { name: 'Retry' }));
    expect(install).toHaveBeenCalledTimes(1);
    publish({ ...availableUpdate(), phase: 'downloading', done: 1, total: 100 });
    expect(screen.queryByText('Could not update to 0.2.0')).not.toBeInTheDocument();
  });

  it('reports a refusal of the backend instead of failing silently', async () => {
    boot(true, availableUpdate());
    vi.spyOn(api, 'updateInstall').mockRejectedValue('busy:installation');
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Update' }));
    expect(await screen.findByText('Finish the installation first.')).toBeInTheDocument();
  });

  it('disables the button, with the reason, while the game runs', async () => {
    boot(true, availableUpdate());
    const install = vi.spyOn(api, 'updateInstall');
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'PLAY' }));
    await screen.findByRole('button', { name: 'RUNNING' });
    expect(screen.getByRole('button', { name: 'Update' })).toBeDisabled();
    expect(screen.getByText('Close the game to update.')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Update' }));
    expect(install).not.toHaveBeenCalled();
    // The mock game closes by itself: the button comes back.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Update' })).toBeEnabled(), { timeout: 5000 });
    expect(screen.queryByText('Close the game to update.')).not.toBeInTheDocument();
  });

  it('disables the button while an installation, an optimization or a texture pack runs', async () => {
    boot(true, availableUpdate());
    await screen.findByRole('button', { name: 'Update' });
    act(() => api.emit('install-progress', { stage: 2, stageProgress: 0.1, recentLines: [], activity: idleActivity }));
    expect(screen.getByRole('button', { name: 'Update' })).toBeDisabled();
    expect(screen.getByText('Finish the installation first.')).toBeInTheDocument();
    act(() => api.emit('install-done', { ok: true, cancelled: false, error: '', elf: '', executable: '', failureKind: null }));
    expect(screen.getByRole('button', { name: 'Update' })).toBeEnabled();

    act(() => api.emit('optimizer', { profiling: false, profileReady: false, profileSupported: false, profileStage: 'none' as const, instrumented: false, activity: idleActivity, configured: true, compiling: true, captured: 1, compiled: 1, completionRatio: 0.5, buildProgress: 0.2, status: 'compiling' }));
    expect(screen.getByRole('button', { name: 'Update' })).toBeDisabled();
    expect(screen.getByText('Wait for the optimization to finish.')).toBeInTheDocument();
    act(() => api.emit('optimizer', { profiling: false, profileReady: false, profileSupported: false, profileStage: 'none' as const, instrumented: false, activity: idleActivity, configured: true, compiling: false, captured: 1, compiled: 1, completionRatio: 1, buildProgress: -1, status: 'idle' }));
    expect(screen.getByRole('button', { name: 'Update' })).toBeEnabled();

    act(() => api.emit('texture-pack-progress', { stage: 'downloading', done: 1, total: 100, overall: 0.1 }));
    expect(screen.getByRole('button', { name: 'Update' })).toBeDisabled();
    expect(screen.getByText('Wait for the texture pack to install.')).toBeInTheDocument();
  });

  it('opens the release page, and says why, for a portable copy', async () => {
    boot(true, availableUpdate('portable'));
    const install = vi.spyOn(api, 'updateInstall');
    const open = vi.spyOn(api, 'updateOpenRelease');
    expect(await screen.findByText('Version 0.2.0 is available')).toBeInTheDocument();
    expect(screen.getByText(/This portable copy cannot update itself/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Update' })).not.toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Open release page' }));
    expect(open).toHaveBeenCalledTimes(1);
    expect(install).not.toHaveBeenCalled();
  });

  it('opens the release page when the release cannot be installed in place', async () => {
    boot(true, availableUpdate('manual'));
    expect(await screen.findByText('Download it from the release page.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open release page' })).toBeEnabled();
  });

  it('is in Portuguese when the language is', async () => {
    boot(true, availableUpdate());
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Other' }));
    await user.click(screen.getByRole('combobox', { name: 'Language' }));
    await user.click(screen.getByRole('option', { name: 'Português (Brasil)' }));
    expect(await screen.findByText('A versão 0.2.0 está disponível')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Atualizar' })).toBeEnabled();
  });
});

describe('automatic updates setting', () => {
  it('is off by default and saved when turned on', async () => {
    boot(true);
    const patch = vi.spyOn(api, 'patchSettings');
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Other' }));
    const toggle = screen.getByRole('switch', { name: 'Automatic updates' });
    expect(toggle).not.toBeChecked();
    await user.click(toggle);
    await waitFor(() => expect(patch).toHaveBeenCalledWith({ autoUpdate: true }));
    expect(screen.getByRole('switch', { name: 'Automatic updates' })).toBeChecked();
  });
});

describe('saves', () => {
  const saves = {
    slots: [
      { index: 0, file: 'data0.bin', modifiedUnix: 1793936917, zone: 'Zone01A', playSeconds: 2902.96, savePoint: 4, difficulty: 0 },
      { index: 3, file: 'data1.bin', modifiedUnix: 1793937000, zone: 'Zone09B', playSeconds: 59, savePoint: 9, difficulty: 2 },
    ],
    art: { places: { '4': 'Place Four' }, difficulties: ['Easy One', 'Normal One'], thumbnails: { '4': 'data:image/png;base64,AAAA' } },
  };

  it('shows each slot with the names and thumbnail read from the disc, and falls back to the zone code', async () => {
    api = createMockApi({ ready: true, saves });
    setApi(api);
    render(<App />);
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Saves' }));
    const cards = await screen.findAllByTestId('save-slot');
    expect(cards).toHaveLength(2);
    expect(within(cards[0]).getByText('Place Four')).toBeInTheDocument();
    expect(within(cards[0]).getByText('Easy One · 0:48:22')).toBeInTheDocument();
    expect(within(cards[0]).getByRole('img', { name: 'Place Four' })).toHaveAttribute('src', 'data:image/png;base64,AAAA');
    // no name, no difficulty name and no thumbnail on the disc for the second one
    expect(within(cards[1]).getByText('Zone Zone09B')).toBeInTheDocument();
    expect(within(cards[1]).getByText('Difficulty 3 · 0:00:59')).toBeInTheDocument();
    expect(within(cards[1]).queryByRole('img')).toBeNull();
  });

  it('loads a slot: with the game closed it asks for it and starts the game, while it runs it asks first', async () => {
    api = createMockApi({ ready: true, saves });
    setApi(api);
    render(<App />);
    const user = userEvent.setup();
    const request = vi.spyOn(api, 'loadSaveSlot');
    const start = vi.spyOn(api, 'startGame');
    await user.click(await screen.findByRole('button', { name: 'Saves' }));
    const cards = await screen.findAllByTestId('save-slot');
    await user.click(within(cards[1]).getByRole('button', { name: 'Load' }));
    await waitFor(() => expect(start).toHaveBeenCalled());
    expect(request).toHaveBeenCalledWith(expect.any(String), 3);
    expect(request.mock.invocationCallOrder[0]).toBeLessThan(start.mock.invocationCallOrder[0]);
    // the game is running now: a second load asks before throwing the progress away
    await screen.findByRole('button', { name: 'RUNNING' });
    await user.click(within(screen.getAllByTestId('save-slot')[0]).getByRole('button', { name: 'Load' }));
    expect(screen.getByText('Load slot 1? Progress since your last save is lost.')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Yes, load' }));
    await waitFor(() => expect(request).toHaveBeenCalledTimes(2));
    expect(request).toHaveBeenLastCalledWith(expect.any(String), 0);
    expect(start).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('The game is going to slot 1.')).toBeInTheDocument();
  });
});
