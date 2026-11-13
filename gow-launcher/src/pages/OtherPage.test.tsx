import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from '../App';
import { createMockApi, setApi, type MockApi } from '../lib/backend';

let api: MockApi;

afterEach(cleanup);

async function openOther() {
  api = createMockApi({ ready: true });
  setApi(api);
  render(<App />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole('button', { name: 'Other' }));
  return user;
}

describe('session report in the Other tab', () => {
  it('shows the export row and the marker row', async () => {
    await openOther();
    expect(screen.getByRole('button', { name: 'Export report' })).toBeEnabled();
    expect(screen.getByRole('textbox', { name: 'Mark a problem' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Mark' })).toBeDisabled();
  });

  it('exports the report and says where it went', async () => {
    const user = await openOther();
    const exported = vi.spyOn(api, 'reportExport');
    await user.click(screen.getByRole('button', { name: 'Export report' }));
    expect(exported).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('Report saved: gow-report-20260101-120000.zip')).toBeInTheDocument();
  });

  it('says so when the export fails', async () => {
    const user = await openOther();
    vi.spyOn(api, 'reportExport').mockRejectedValue(new Error('disk full'));
    await user.click(screen.getByRole('button', { name: 'Export report' }));
    expect(await screen.findByText('The report could not be saved.')).toBeInTheDocument();
  });

  it('sends a note and empties the field', async () => {
    const user = await openOther();
    const mark = vi.spyOn(api, 'reportMark');
    const field = screen.getByRole('textbox', { name: 'Mark a problem' });
    await user.type(field, 'fell through the floor');
    await user.click(screen.getByRole('button', { name: 'Mark' }));
    expect(mark).toHaveBeenCalledWith('fell through the floor');
    await waitFor(() => expect(field).toHaveValue(''));
    expect(await screen.findByText('Marked for the next report.')).toBeInTheDocument();
  });
});

describe('session report in the failure dialog', () => {
  it('offers the export and the note next to the log', async () => {
    api = createMockApi({ ready: true });
    setApi(api);
    render(<App />);
    await screen.findByRole('button', { name: 'PLAY' });
    act(() => api.emit('game-finished', { code: 3, failed: true, seconds: 3, summary: 'boom', failureKind: null }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByRole('button', { name: 'Open log' })).toBeInTheDocument();
    expect(within(dialog).getByRole('textbox', { name: 'Mark a problem' })).toBeInTheDocument();
    const exported = vi.spyOn(api, 'reportExport');
    await userEvent.setup().click(within(dialog).getByRole('button', { name: 'Export report' }));
    expect(exported).toHaveBeenCalledTimes(1);
  });
});

describe('starting the game directly (issue #40)', () => {
  it('saves the switch', async () => {
    const user = await openOther();
    const patched = vi.spyOn(api, 'patchSettings');
    const toggle = screen.getByRole('switch', { name: 'Start the game directly' });
    expect(toggle).toHaveAttribute('aria-checked', 'false');
    await user.click(toggle);
    expect(toggle).toHaveAttribute('aria-checked', 'true');
    await waitFor(() => expect(patched).toHaveBeenCalledWith(expect.objectContaining({ skipLauncher: true })));
    expect(screen.getByText(/Hold Shift while opening it/)).toBeInTheDocument();
  });

  it('copies the launcher path for Steam', async () => {
    const user = await openOther();
    const written = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: written }, configurable: true });
    await user.click(screen.getByRole('button', { name: 'Copy path' }));
    expect(written).toHaveBeenCalledWith(expect.stringContaining('gow-launcher.exe'));
    expect(await screen.findByText("The launcher's path was copied.")).toBeInTheDocument();
  });

  it('shows the path when it cannot be copied', async () => {
    const user = await openOther();
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: vi.fn().mockRejectedValue(new Error('denied')) }, configurable: true });
    await user.click(screen.getByRole('button', { name: 'Copy path' }));
    expect(await screen.findByText(/Could not copy\. The launcher is at .*gow-launcher\.exe/)).toBeInTheDocument();
  });

  it('shows why the window opened when the game failed after a direct start', async () => {
    api = createMockApi({
      ready: true,
      notice: { finished: { code: -1, failed: true, seconds: 3, summary: 'the last lines', failureKind: null }, launchError: null },
    });
    setApi(api);
    render(<App />);
    expect(await screen.findByText('The game closed unexpectedly (code -1).')).toBeInTheDocument();
    expect(screen.getByText('the last lines')).toBeInTheDocument();
  });

  it('shows a launch error the same way PLAY does', async () => {
    api = createMockApi({ ready: true, notice: { finished: null, launchError: { key: 'errorInstrumented', detail: 'C:\\gow-2005.exe' } } });
    setApi(api);
    render(<App />);
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText(/gow-2005\.exe/)).toBeInTheDocument();
  });

  it('shows a notice that arrives while the window is open', async () => {
    api = createMockApi({ ready: true });
    setApi(api);
    render(<App />);
    await screen.findByRole('button', { name: 'PLAY' });
    act(() => api.emit('start-notice', { finished: { code: 3, failed: true, seconds: 1, summary: '', failureKind: null }, launchError: null }));
    expect(await screen.findByText('The game closed unexpectedly (code 3).')).toBeInTheDocument();
  });
});
