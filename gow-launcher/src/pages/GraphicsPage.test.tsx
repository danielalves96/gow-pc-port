import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from '../App';
import { createMockApi, setApi } from '../lib/backend';

afterEach(cleanup);

async function openGraphics() {
  const api = createMockApi({ ready: true });
  setApi(api);
  render(<App />);
  const user = userEvent.setup();
  const patch = vi.spyOn(api, 'patchSettings');
  const preset = await screen.findByRole('combobox', { name: 'Graphics preset' });
  return { user, patch, preset };
}

describe('graphics presets in the Graphics tab', () => {
  it('shows Balanced on the defaults and writes only its four options when Quality is chosen', async () => {
    const { user, patch, preset } = await openGraphics();
    expect(preset).toHaveTextContent('Balanced');
    expect(screen.queryByText('Above 2× is heavy')).not.toBeInTheDocument();
    await user.click(preset);
    await user.click(screen.getByRole('option', { name: /^Quality/ }));
    // The change reaches the backend after the debounce, as one patch with the four options and nothing else.
    await waitFor(() => expect(patch).toHaveBeenCalledWith({ internalResolution: '3', filter: 'fsr', sharpness: 0.8, fxaa: true }));
    // The heavy-scale warning appears with the 3× scale, and the preset still reads as Quality.
    expect(await screen.findByText('Above 2× is heavy')).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Graphics preset' })).toHaveTextContent('Quality');
  });

  it('turns into Custom after a manual change, and Custom cannot be picked', async () => {
    const { user, preset } = await openGraphics();
    await user.click(screen.getByRole('switch', { name: 'Anti-aliasing (FXAA)' }));
    expect(await screen.findByRole('combobox', { name: 'Graphics preset' })).toHaveTextContent('Custom');
    await user.click(preset);
    expect(screen.getByRole('option', { name: 'Custom' })).toHaveAttribute('aria-disabled', 'true');
    await user.click(screen.getByRole('option', { name: /^Performance/ }));
    expect(await screen.findByRole('combobox', { name: 'Graphics preset' })).toHaveTextContent('Performance');
  });
});

describe('image adjustment in the Graphics tab', () => {
  it('starts at 50 with Reset disabled, writes a moved slider and resets all four to 50', async () => {
    const { user, patch } = await openGraphics();
    const reset = screen.getByRole('button', { name: 'Reset' });
    expect(reset).toBeDisabled();
    for (const name of ['Brightness', 'Contrast', 'Gamma', 'Saturation']) {
      expect(screen.getByRole('slider', { name })).toHaveValue('50');
    }
    fireEvent.change(screen.getByRole('slider', { name: 'Gamma' }), { target: { value: '70' } });
    await waitFor(() => expect(patch).toHaveBeenCalledWith({ gamma: 70 }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Reset' })).toBeEnabled());
    await user.click(screen.getByRole('button', { name: 'Reset' }));
    await waitFor(() => expect(patch).toHaveBeenCalledWith({ brightness: 50, contrast: 50, gamma: 50, saturation: 50 }));
    expect(await screen.findByRole('slider', { name: 'Gamma' })).toHaveValue('50');
  });
});

describe('render resolution and the in-game overlay', () => {
  it('offers render resolutions from a quarter of the PS2 up to 8x', async () => {
    const { user, patch } = await openGraphics();
    await user.click(screen.getByRole('combobox', { name: 'Render resolution' }));
    expect(screen.getAllByRole('option', { name: /^\d+(\.\d+)?×|^Native/ })).toHaveLength(13);
    expect(screen.getByRole('option', { name: /^0\.25×/ })).toBeInTheDocument();
    expect(screen.queryByRole('option', { name: /^9×/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole('option', { name: /^8×/ }));
    await waitFor(() => expect(patch).toHaveBeenCalledWith({ internalResolution: '8' }));
  });

  it('has one switch per part of the overlay on the Performance tab', async () => {
    const { user, patch } = await openGraphics();
    await user.click(screen.getByRole('button', { name: /Performance/ }));
    for (const name of ['FPS counter', 'GPU', 'CPU', 'RAM']) {
      expect(await screen.findByRole('switch', { name })).not.toBeChecked();
    }
    await user.click(screen.getByRole('switch', { name: 'CPU' }));
    await waitFor(() => expect(patch).toHaveBeenCalledWith({ showCpu: true }));
    expect(screen.getByRole('switch', { name: 'FPS counter' })).not.toBeChecked();
    expect(screen.queryByText(/120 FPS \(experimental\)/)).not.toBeInTheDocument();
  });
});
