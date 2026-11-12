import { useMemo } from 'react';
import { Activity, Cpu, Flame, Gauge, Gpu, Hourglass, Layers, MemoryStick, Microchip, RefreshCw, Rocket } from 'lucide-react';
import { BuildDetailsButton } from '../components/Shell';
import { GroupTitle, Note, ProgressBar, SecondaryButton, Select, SettingRow, Switch, type Option } from '../components/ui';
import { useApp } from '../state/app';

/** Highest internal scale the launcher offers (`MAX_SCALE` in gow-core). */
export const MAX_SCALE = 8;

/** Lowest internal scale the launcher offers (`MIN_SCALE` in gow-core). */
export const MIN_SCALE = 0.25;

/** Internal scale requested: "auto" and anything above 8x fall back to 2x (mirrors `Settings::selected_scale`). */
export function selectedScale(internalResolution: string): number {
  const n = parseFloat(internalResolution);
  return internalResolution === 'auto' || Number.isNaN(n) || n > MAX_SCALE ? 2 : Math.max(MIN_SCALE, n);
}

export function PerformancePage() {
  const { settings, patch, optimizer, info, api, t, f, rebuild, rebuildBlocker, startRebuild, requestProfile } = useApp();
  const optimizing = settings.optimizationDestination.length > 0;
  const { graphicsCards, platform } = info;
  const gpuUnavailable = settings.gpu !== 'auto' && !graphicsCards.some((card) => card.id === settings.gpu);
  const gpuOptions = useMemo<Option[]>(
    () => [
      { text: t('gpuAuto'), value: 'auto' },
      ...graphicsCards.map((card) => ({
        text:
          platform === 'windows'
            ? card.id === 'ahorro' ? t('gpuPowerSaver') : t('gpuPerformance')
            : `${card.manufacturer} ${card.displayName}`,
        value: card.id,
        detail: (card.kind ? t(card.kind) : '') + (platform === 'linux' ? `  ·  ${card.id}` : ''),
      })),
    ],
    [graphicsCards, platform, t],
  );
  return (
    <div className="flex flex-col gap-1">
      {optimizer.status === 'error' && (
        <div className="mb-2">
          <Note tone="warn" title={t('rebuildFailedTitle')}>
            <p>{t('rebuildFailedText')}</p>
            <div className="mt-2.5">
              <SecondaryButton onClick={() => void api.openOptimizerLog()}>{t('rebuildFailedLog')}</SecondaryButton>
            </div>
          </Note>
        </div>
      )}
      {rebuild.needed && (
        <div className="mb-2">
          <Note tone="warn" title={t(rebuild.legacy ? 'rebuildLegacyTitle' : 'rebuildNoteTitle')}>
            <p>{t(rebuild.legacy ? 'rebuildLegacyText' : 'rebuildNoteText')}</p>
            {rebuild.legacy && <p className="mt-1.5">{t('rebuildLegacySaves')}</p>}
            {optimizer.compiling ? (
              <div role="status" className="mt-2.5 flex items-center gap-3">
                <span className="text-ink-muted">
                  {optimizer.buildProgress >= 0 ? f('rebuildProgress', Math.min(100, Math.round(optimizer.buildProgress * 100))) : t('rebuildRunning')}
                </span>
                <ProgressBar label={t('rebuildProgressLabel')} fraction={optimizer.buildProgress} className="w-[220px]" />
                <BuildDetailsButton />
              </div>
            ) : (
              <div className="mt-2.5 flex items-center gap-3">
                <SecondaryButton disabled={rebuildBlocker !== null} onClick={() => void startRebuild()}>
                  {t(rebuild.legacy ? 'rebuildLegacyButton' : 'rebuildButton')}
                </SecondaryButton>
                {rebuildBlocker && <span className="text-ink-muted">{t(`rebuildBlocked_${rebuildBlocker}`)}</span>}
              </div>
            )}
          </Note>
        </div>
      )}
      <GroupTitle>{t('groupRenderer')}</GroupTitle>
      <SettingRow icon={MemoryStick} title={t('gpu')} description={gpuUnavailable ? t('gpuMissing') : t('gpuDesc')}>
        <Select label={t('gpu')} options={gpuOptions} value={settings.gpu} onChange={(gpu) => patch({ gpu })} />
      </SettingRow>
      <GroupTitle>{t('groupFrameRate')}</GroupTitle>
      <SettingRow icon={Gauge} title={t('fpsLimit')} description={t('fpsLimitDesc')}>
        <Select
          label={t('fpsLimit')}
          options={[{ text: t('fps60'), value: 60 }, { text: t('fps120'), value: 120 }]}
          value={settings.fpsLimit}
          onChange={(fpsLimit) => patch({ fpsLimit })}
        />
      </SettingRow>
      <SettingRow icon={RefreshCw} title={t('vsync')} description={t('vsyncDesc')}>
        <Switch label={t('vsync')} checked={settings.vsync} onChange={(vsync) => patch({ vsync })} />
      </SettingRow>
      {settings.vsync && (
        <Note tone="warn" title={t('vsyncWarningTitle')}>
          {t('vsyncWarning')}
        </Note>
      )}
      <SettingRow icon={Layers} title={t('earlyFinish')} description={t('earlyFinishDesc')}>
        <Switch label={t('earlyFinish')} checked={settings.earlyFinish} onChange={(earlyFinish) => patch({ earlyFinish })} />
      </SettingRow>
      <GroupTitle>{t('groupEffects')}</GroupTitle>
      <SettingRow icon={Layers} title={t('fxShadows')} description={t('fxShadowsDesc')}>
        <Switch label={t('fxShadows')} checked={settings.fxShadows} onChange={(fxShadows) => patch({ fxShadows })} />
      </SettingRow>
      <SettingRow icon={Flame} title={t('fxAdditive')} description={t('fxAdditiveDesc')}>
        <Switch label={t('fxAdditive')} checked={settings.fxAdditive} onChange={(fxAdditive) => patch({ fxAdditive })} />
      </SettingRow>
      <SettingRow icon={Gpu} title={t('fxFilter')} description={t('fxFilterDesc')}>
        <Switch label={t('fxFilter')} checked={settings.fxFilter} onChange={(fxFilter) => patch({ fxFilter })} />
      </SettingRow>
      <GroupTitle>{t('groupOverlay')}</GroupTitle>
      <SettingRow icon={Activity} title={t('showFps')} description={t('showFpsDesc')}>
        <Switch label={t('showFps')} checked={settings.showFps} onChange={(showFps) => patch({ showFps })} />
      </SettingRow>
      <SettingRow icon={Gpu} title={t('showGpu')} description={t('showGpuDesc')}>
        <Switch label={t('showGpu')} checked={settings.showGpu} onChange={(showGpu) => patch({ showGpu })} />
      </SettingRow>
      <SettingRow icon={Microchip} title={t('showCpu')} description={t('showCpuDesc')}>
        <Switch label={t('showCpu')} checked={settings.showCpu} onChange={(showCpu) => patch({ showCpu })} />
      </SettingRow>
      <SettingRow icon={MemoryStick} title={t('showRam')} description={t('showRamDesc')}>
        <Switch label={t('showRam')} checked={settings.showRam} onChange={(showRam) => patch({ showRam })} />
      </SettingRow>
      {optimizing && <GroupTitle>{t('groupOptimization')}</GroupTitle>}
      <SettingRow hidden={!optimizing} icon={Cpu} title={t('optimizationEnabled')} description={t('optimizationEnabledDesc')}>
        <Switch label={t('optimizationEnabled')} checked={settings.optimizationEnabled} onChange={(optimizationEnabled) => patch({ optimizationEnabled })} />
      </SettingRow>
      <SettingRow hidden={!optimizing} disabled={!settings.optimizationEnabled} icon={Hourglass} title={t('optimizeDuringGame')} description={t('optimizeDuringGameDesc')}>
        <Switch label={t('optimizeDuringGame')} checked={settings.optimizeDuringGame} onChange={(optimizeDuringGame) => patch({ optimizeDuringGame })} />
      </SettingRow>
      {optimizing && settings.optimizationEnabled && settings.optimizeDuringGame && (
        <Note tone="warn" title={t('optimizeDuringGameWarningTitle')}>
          {t('optimizeDuringGameWarning')}
        </Note>
      )}
      <SettingRow
        hidden={!(optimizing && optimizer.configured)}
        icon={Rocket} title={t('optimizeNow')}
        description={f('optimizeNowDesc', optimizer.compiled, optimizer.captured)}
      >
        <SecondaryButton disabled={optimizer.status === 'compiling'} onClick={() => void api.optimizeNow()}>
          {t('optimizeNowButton')}
        </SecondaryButton>
      </SettingRow>
      <SettingRow
        hidden={!(optimizing && optimizer.profileSupported)}
        icon={Flame} title={t('profileTitle')}
        description={t(
          optimizer.profiling
            ? 'profileRunningDesc'
            : optimizer.instrumented
              ? 'instrumentedDesc'
              : optimizer.profileReady
                ? 'profileReadyDesc'
                : 'profileOfferDesc',
        )}
      >
        {optimizer.profiling ? (
          <ProgressBar label={t('profileProgressLabel')} fraction={optimizer.buildProgress} className="w-[160px]" />
        ) : optimizer.profileReady || optimizer.instrumented ? null : (
          <SecondaryButton
            disabled={optimizer.compiling || rebuildBlocker !== null}
            title={rebuildBlocker ? t(`rebuildBlocked_${rebuildBlocker}`) : undefined}
            onClick={requestProfile}
          >
            {t('profileButton')}
          </SecondaryButton>
        )}
      </SettingRow>
      <div className="mt-[18px]">
        <Note title={t('noteFpsTitle')}>{t('noteFps')}</Note>
      </div>
    </div>
  );
}
