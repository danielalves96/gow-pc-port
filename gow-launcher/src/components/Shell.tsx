import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import {
  AlertTriangle, Check, Circle, Gauge, Gamepad2, FolderOpen, Layers, Loader2, Minus, Monitor, Play, Save, Settings2, Sparkles, Square, Trophy, X,
} from 'lucide-react';
import badgeUrl from '../assets/theme/badge.webp';
import logoUrl from '../assets/theme/logo.webp';
import type { BuildActivity, ProfileStage } from '../lib/types';
import { useApp } from '../state/app';
import { MarkField, useReportExport } from './ReportTools';
import { PrimaryButton, ProgressBar, SecondaryButton, type IconComponent } from './ui';

// ----------------------------------------------------------------------------------------------- title bar

export function TitleBar() {
  const { api, t } = useApp();
  return (
    <header
      data-tauri-drag-region
      className="relative z-[60] flex h-[34px] shrink-0 items-center justify-between border-b border-[#5a3d22] bg-gradient-to-r from-[#17100b]/90 via-[#2c1d12]/80 to-[#3a1d14]/70"
    >
      <div data-tauri-drag-region className="flex items-center gap-2.5 pl-3">
        <img src={badgeUrl} alt="" width={22} height={22} className="pointer-events-none" />
        <span data-tauri-drag-region className="text-[11px] font-semibold tracking-[0.18em] text-[#d9c6a3]">
          GOD OF WAR (2005)
        </span>
      </div>
      {/* Same place in every section, and clear of the window controls: the title bar is the one strip that never changes. */}
      <div data-tauri-drag-region className="flex h-full min-w-0 flex-1 items-center justify-center px-4">
        <UpdateNotice />
      </div>
      <div className="flex h-full">
        {[
          { id: 'minimize', icon: Minus, size: 16, run: () => api.minimize() },
          { id: 'maximize', icon: Square, size: 12, run: () => api.toggleMaximize() },
          { id: 'close', icon: X, size: 16, run: () => api.close() },
        ].map(({ id, icon: Icon, size, run }) => (
          <button
            key={id}
            type="button"
            title={t(id)}
            aria-label={t(id)}
            onClick={() => void run()}
            className={`flex h-full w-[46px] items-center justify-center text-[#e7d6b4] transition-colors ${id === 'close' ? 'hover:bg-[#a3190f]' : 'hover:bg-white/10'}`}
          >
            <Icon size={size} />
          </button>
        ))}
      </div>
    </header>
  );
}

// ----------------------------------------------------------------------------------------------- update notice

const noticeButton =
  'h-[22px] shrink-0 whitespace-nowrap rounded border border-gold bg-[#4a2f18] px-2.5 text-[11px] font-semibold tracking-wide text-cream transition-colors hover:bg-[#6a4424] disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:bg-[#4a2f18]';

/**
 * A newer launcher release, in the title bar. It says what the button will do (update here, or open the release page for
 * a copy that cannot update itself), shows the progress while it runs and a failure with a retry, and explains a disabled
 * button instead of just greying it. Nothing at all while there is nothing to offer.
 */
export function UpdateNotice() {
  const { update, updateBlocker, installUpdate, openUpdatePage, f, t } = useApp();
  if (update.phase === 'idle') return null;
  const { phase, mode, version } = update;
  const text = 'min-w-0 truncate text-[11px] font-semibold tracking-wide text-[#e7d6b4]';
  const muted = 'min-w-0 truncate text-[11px] text-[#b9a27a]';

  if (phase === 'downloading' || phase === 'installing') {
    const known = update.total > 0;
    const percent = phase === 'installing' ? 100 : known ? Math.min(100, Math.floor((update.done / update.total) * 100)) : 0;
    const label = phase === 'installing' ? t('updateInstalling') : known ? f('updateDownloading', percent) : t('updateDownloadingUnknown');
    return (
      <div role="status" className="flex min-w-0 items-center gap-3" title={label}>
        <span className={text}>{label}</span>
        <div
          role="progressbar"
          aria-label={t('updateProgress')}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={known || phase === 'installing' ? percent : undefined}
          className="h-[6px] w-[150px] shrink-0 overflow-hidden rounded-full bg-[#140b06]/70"
        >
          <div
            className={`h-full rounded-full bg-gold-light ${known || phase === 'installing' ? '' : 'w-1/3 animate-pulse'}`}
            style={known || phase === 'installing' ? { width: `${percent}%` } : undefined}
          />
        </div>
      </div>
    );
  }

  if (phase === 'error' && mode === 'automatic') {
    return (
      <div role="status" className="flex min-w-0 items-center gap-3" title={update.error}>
        <span className={`${text} text-[#f0b4a8]`}>{f('updateFailed', version)}</span>
        <button type="button" className={noticeButton} disabled={updateBlocker !== null} onClick={() => void installUpdate()}>
          {t('updateRetry')}
        </button>
        {updateBlocker && <span className={muted}>{t(`updateBlocked_${updateBlocker}`)}</span>}
      </div>
    );
  }

  if (mode === 'automatic') {
    const reason = updateBlocker ? t(`updateBlocked_${updateBlocker}`) : '';
    return (
      <div role="status" className="flex min-w-0 items-center gap-3" title={reason}>
        <span className={text}>{f('updateAvailable', version)}</span>
        {reason && <span className={muted}>{reason}</span>}
        <button type="button" className={noticeButton} disabled={updateBlocker !== null} onClick={() => void installUpdate()}>
          {t('updateButton')}
        </button>
      </div>
    );
  }

  // A portable copy (or a package install) cannot replace itself: the button opens the page where the new one is.
  const why = t(mode === 'portable' ? 'updatePortableNote' : 'updateManualNote');
  return (
    <div role="status" className="flex min-w-0 items-center gap-3" title={`${f('updateAvailable', version)}. ${why}`}>
      <span className={text}>{f('updateAvailable', version)}</span>
      <span className={muted}>{why}</span>
      <button type="button" className={noticeButton} onClick={() => void openUpdatePage()}>
        {t('updateOpenPage')}
      </button>
    </div>
  );
}

// ----------------------------------------------------------------------------------------------- hero

/** Small parchment label for the information that sits over the banner art, so it stays legible on any part of it. */
const chip = 'whitespace-nowrap rounded-md border border-line bg-[#eadbb9]/95 px-3 py-1 shadow-[0_1px_3px_rgba(40,20,5,0.35)]';

/**
 * The rebuild the last update asked for, on the first screen: a notice with the button while it is waiting, the progress
 * while it runs. Nothing when the build is up to date (a background optimization alone stays quiet). The same state and
 * the same button as the Performance tab, so the player does not have to know where to look.
 */
export function RebuildChip() {
  const { rebuild, rebuildBlocker, startRebuild, optimizer, f, t } = useApp();
  const [confirming, setConfirming] = useState(false);
  if (!rebuild.needed) return null;
  const chipRow = `${chip} flex items-center gap-2 text-[12.5px] font-semibold text-ink`;
  if (optimizer.compiling) {
    const known = optimizer.buildProgress >= 0;
    return (
      <div role="status" className={chipRow}>
        <span>{known ? f('rebuildProgress', Math.min(100, Math.round(optimizer.buildProgress * 100))) : t('rebuildRunning')}</span>
        <ProgressBar label={t('rebuildProgressLabel')} fraction={optimizer.buildProgress} className="w-[110px]" />
        <BuildDetailsButton />
      </div>
    );
  }
  return (
    <div role="status" className={chipRow} title={rebuildBlocker ? t(`rebuildBlocked_${rebuildBlocker}`) : undefined}>
      <span>{t(rebuild.legacy ? 'rebuildLegacyChip' : 'rebuildChip')}</span>
      <button
        type="button"
        disabled={rebuildBlocker !== null}
        // GOW-Port (0.3.0): a build of 0.2.x is deleted and built from scratch: the player confirms first.
        onClick={() => (rebuild.legacy ? setConfirming(true) : void startRebuild())}
        className="h-[22px] rounded border border-gold bg-[#4a2f18] px-2.5 text-[11px] font-semibold tracking-wide text-cream transition-colors hover:bg-[#6a4424] disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:bg-[#4a2f18]"
      >
        {t('rebuildChipButton')}
      </button>
      {confirming && (
        <LegacyRebuildDialog
          onConfirm={() => {
            setConfirming(false);
            void startRebuild();
          }}
          onCancel={() => setConfirming(false)}
        />
      )}
    </div>
  );
}

/**
 * GOW-Port (0.3.0): the build was made by 0.2.9 or older, with Visual Studio. 0.3.0 compiles the game with clang, so the old
 * build is deleted and the game built from scratch; the saves and the memory card are not in the build folder and stay.
 */
export function LegacyRebuildDialog({ onConfirm, onCancel }: { onConfirm: () => void; onCancel: () => void }) {
  const { t } = useApp();
  return (
    <Modal>
      <div className="flex flex-col gap-3.5">
        <h2 className="gow-text-shadow font-title text-[18px] font-bold uppercase tracking-wide text-ink">{t('rebuildLegacyTitle')}</h2>
        <p className="text-[15px] font-medium text-ink">{t('rebuildLegacyText')}</p>
        <p className="text-[15px] font-medium text-ink">{t('rebuildLegacySaves')}</p>
        <div className="flex justify-end gap-2">
          <SecondaryButton onClick={onCancel}>{t('rebuildLegacyLater')}</SecondaryButton>
          <PrimaryButton onClick={onConfirm}>{t('rebuildLegacyButton')}</PrimaryButton>
        </div>
      </div>
    </Modal>
  );
}

/**
 * GOW-Port: the offer of the speed-profile optimization on the first screen, once the game is installed with clang and
 * has no profile yet. It says what will happen (the game opens by itself, do not use the PC) before anything starts;
 * "Later" hides it on this PC, and the Performance tab keeps the button. While it runs, the progress takes its place.
 */
export function ProfileChip() {
  const { settings, optimizer, rebuild, rebuildBlocker, requestProfile, profileOfferDismissed, dismissProfileOffer, t } = useApp();
  const chipRow = `${chip} flex items-center gap-2 text-[12.5px] font-semibold text-ink`;
  if (rebuild.needed || optimizer.instrumented || !settings.optimizationDestination || !optimizer.profileSupported || optimizer.profileReady) {
    return null;
  }
  if (optimizer.profiling) {
    return (
      <div role="status" className={chipRow}>
        <span>{t('profileChipRunning')}</span>
        <ProgressBar label={t('profileProgressLabel')} fraction={optimizer.buildProgress} className="w-[110px]" />
      </div>
    );
  }
  if (profileOfferDismissed || optimizer.compiling) return null;
  const small = 'h-[22px] rounded border px-2.5 text-[11px] font-semibold tracking-wide transition-colors disabled:cursor-not-allowed disabled:opacity-45';
  return (
    <div role="status" className={chipRow} title={t('profileOfferDesc')}>
      <span>{t('profileChip')}</span>
      <button
        type="button"
        disabled={rebuildBlocker !== null}
        onClick={requestProfile}
        className={`${small} border-gold bg-[#4a2f18] text-cream hover:bg-[#6a4424] disabled:hover:bg-[#4a2f18]`}
      >
        {t('profileButton')}
      </button>
      <button type="button" onClick={dismissProfileOffer} className={`${small} border-line bg-transparent text-ink hover:text-accent`}>
        {t('profileLater')}
      </button>
    </div>
  );
}

const profileSteps = [
  { stage: 'building', title: 'profileStepBuilding', hint: 'profileStepBuildingHint' },
  { stage: 'training', title: 'profileStepTraining', hint: 'profileStepTrainingHint' },
  { stage: 'finishing', title: 'profileStepFinishing', hint: 'profileStepFinishingHint' },
] as const;

/** The three steps of "Optimize now", each with what the player may do meanwhile; `current` marks where the run is. */
function ProfileSteps({ current }: { current?: ProfileStage }) {
  const { t } = useApp();
  const at = current ? profileSteps.findIndex((step) => step.stage === current) : -1;
  return (
    <ol className="flex flex-col gap-2.5">
      {profileSteps.map((step, index) => {
        const done = at > index;
        const active = at === index;
        const Icon = done ? Check : active ? Loader2 : Circle;
        return (
          <li key={step.stage} aria-current={active ? 'step' : undefined} className={`flex gap-2.5 ${at >= 0 && !active ? 'opacity-60' : ''}`}>
            <Icon size={18} className={`mt-[1px] shrink-0 ${active ? 'animate-spin text-accent' : 'text-ink'}`} aria-hidden="true" />
            <div className="flex flex-col">
              <span className="text-[14.5px] font-semibold text-ink">{`${index + 1}. ${t(step.title)}`}</span>
              <span className="text-[13px] text-ink-soft">{t(step.hint)}</span>
            </div>
          </li>
        );
      })}
    </ol>
  );
}

/**
 * GOW-Port: "Optimize now" never starts with one click. The player reads what will happen (the game opens by itself and must
 * not be touched, PLAY stays locked, closing the launcher cancels it) and confirms.
 */
export function ProfileConfirmDialog() {
  const { profileConfirmOpen, closeProfileConfirm, startProfile, t } = useApp();
  if (!profileConfirmOpen) return null;
  return (
    <Modal>
      <div className="flex flex-col gap-3.5">
        <h2 className="gow-text-shadow font-title text-[18px] font-bold uppercase tracking-wide text-ink">{t('profileConfirmTitle')}</h2>
        <p className="text-[15px] font-medium text-ink">{t('profileConfirmIntro')}</p>
        <ProfileSteps />
        <p className="rounded border border-line bg-[#4a2f18]/10 px-3 py-2 text-[13.5px] font-medium text-ink">{t('profileConfirmRules')}</p>
        <div className="flex justify-end gap-2">
          <SecondaryButton onClick={closeProfileConfirm}>{t('profileConfirmLater')}</SecondaryButton>
          <PrimaryButton
            onClick={() => {
              closeProfileConfirm();
              void startProfile();
            }}
          >
            {t('profileConfirmStart')}
          </PrimaryButton>
        </div>
      </div>
    </Modal>
  );
}

/**
 * GOW-Port: while "Optimize now" runs nothing else in the launcher can be used (only the title bar stays reachable, to
 * minimize): PLAY would cancel it, and an interrupted run used to leave its slow training build as the game. The overlay
 * says which step it is in and, while the game is open by itself, that nothing may be touched.
 */
export function ProfileOverlay() {
  const { optimizer, cancelProfile, t } = useApp();
  const [confirming, setConfirming] = useState(false);
  if (!optimizer.profiling) return null;
  const training = optimizer.profileStage === 'training';
  return (
    <Modal width={600}>
      <div className="flex flex-col gap-3.5" role="status" aria-live="polite">
        <h2 className="gow-text-shadow font-title text-[18px] font-bold uppercase tracking-wide text-ink">{t('profileOverlayTitle')}</h2>
        <ProfileSteps current={optimizer.profileStage === 'none' ? 'building' : optimizer.profileStage} />
        {training ? (
          <div role="alert" className="flex gap-2.5 rounded border-2 border-[#a3190f] bg-[#a3190f]/10 px-3 py-2.5">
            <AlertTriangle size={22} className="mt-[1px] shrink-0 text-[#a3190f]" aria-hidden="true" />
            <div className="flex flex-col gap-0.5">
              <span className="text-[15px] font-bold uppercase tracking-wide text-[#a3190f]">{t('profileTrainingWarningTitle')}</span>
              <span className="text-[13.5px] font-medium text-ink">{t('profileTrainingWarning')}</span>
            </div>
          </div>
        ) : (
          <ProgressBar label={t('profileProgressLabel')} fraction={optimizer.buildProgress} />
        )}
        <p className="text-[13px] text-ink-soft">{t('profileOverlayFoot')}</p>
        {confirming ? (
          <div className="flex flex-col gap-2.5 rounded border border-line px-3 py-2.5">
            <p className="text-[14px] font-medium text-ink">{t('profileCancelConfirm')}</p>
            <div className="flex justify-end gap-2">
              <SecondaryButton onClick={() => setConfirming(false)}>{t('profileCancelKeep')}</SecondaryButton>
              <SecondaryButton
                danger
                onClick={() => {
                  setConfirming(false);
                  void cancelProfile();
                }}
              >
                {t('profileCancelYes')}
              </SecondaryButton>
            </div>
          </div>
        ) : (
          <div className="flex items-center justify-end gap-2">
            <BuildDetailsButton className="h-[34px] px-3.5 text-[12.5px]" />
            <SecondaryButton onClick={() => setConfirming(true)}>{t('profileCancel')}</SecondaryButton>
          </div>
        )}
      </div>
    </Modal>
  );
}

/**
 * GOW-Port: the installation holds the slow training build of an interrupted "Optimize now". PLAY is locked (the backend
 * refuses it too) and the optimizer rebuilds the game by itself; this says so and, if that rebuild is not running (it failed,
 * or waits after a failure), offers it.
 */
export function InstrumentedChip() {
  const { optimizer, rebuild, rebuildBlocker, startRebuild, t } = useApp();
  // A rebuild the last update asked for is the same build: its own chip already shows it.
  if (!optimizer.instrumented || optimizer.profiling || rebuild.needed) return null;
  const chipRow = `${chip} flex items-center gap-2 text-[12.5px] font-semibold text-ink`;
  if (optimizer.compiling) {
    return (
      <div role="status" className={chipRow} title={t('instrumentedDesc')}>
        <span>{t('instrumentedChipBuilding')}</span>
        <ProgressBar label={t('rebuildProgressLabel')} fraction={optimizer.buildProgress} className="w-[110px]" />
        <BuildDetailsButton />
      </div>
    );
  }
  return (
    <div role="status" className={chipRow} title={t('instrumentedDesc')}>
      <span>{t('instrumentedChip')}</span>
      <button
        type="button"
        disabled={rebuildBlocker !== null}
        onClick={() => void startRebuild()}
        className="h-[22px] rounded border border-gold bg-[#4a2f18] px-2.5 text-[11px] font-semibold tracking-wide text-cream transition-colors hover:bg-[#6a4424] disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:bg-[#4a2f18]"
      >
        {t('instrumentedRebuild')}
      </button>
    </div>
  );
}

/**
 * Play was pressed while the game is being rebuilt. By default the rebuild is paused when the game opens (it would take the
 * processor and the picture would freeze) and starts again when the game closes; with "Optimize during game" it keeps running.
 * Either way the player is told before it happens.
 */
export function PlayDuringRebuildDialog({ onPlay, onWait }: { onPlay: () => void; onWait: () => void }) {
  const { settings, t } = useApp();
  const keeps = settings.optimizeDuringGame;
  return (
    <Modal>
      <div className="flex flex-col gap-3.5">
        <h2 className="gow-text-shadow font-title text-[18px] font-bold uppercase tracking-wide text-ink">{t('playRebuildTitle')}</h2>
        <p className="text-[15px] font-medium text-ink">{t(keeps ? 'playRebuildKeepsText' : 'playRebuildPausesText')}</p>
        <div className="flex justify-end gap-2">
          <SecondaryButton onClick={onWait}>{t('playRebuildWait')}</SecondaryButton>
          <PrimaryButton onClick={onPlay}>{t(keeps ? 'playRebuildPlayAnyway' : 'playRebuildPlayPause')}</PrimaryButton>
        </div>
      </div>
    </Modal>
  );
}

export function Hero() {
  const { status, gameRunning, play, stop, openWizard, textureInstall, rebuild, optimizer, t } = useApp();
  const [asking, setAsking] = useState(false);
  const ready = status?.ready ?? false;
  // The same condition as the notice next to the button: a rebuild the last update asked for is running.
  const rebuilding = rebuild.needed && optimizer.compiling;
  // The textures being written are the ones the game would read.
  const installingTextures = textureInstall.active;
  // GOW-Port: "Optimize now" opens its own game, and its training build is never played (see InstrumentedChip).
  const lockedByProfile = optimizer.profiling || optimizer.instrumented;
  return (
    <section className="relative flex h-[204px] shrink-0 items-stretch justify-between gap-[22px] overflow-hidden pl-[72px] pr-[30px] pb-2 pt-3">
      <div className="gow-glow relative flex min-w-0 flex-col justify-center">
        <img src={logoUrl} alt="God of War" width={430} className="h-auto w-[430px] max-w-full drop-shadow-[0_3px_3px_rgba(40,15,5,0.4)]" />
        {/* Out of the flow, so the logo stays centred in the banner. */}
        <div className="absolute bottom-1.5 left-0 flex w-max items-end gap-2">
          {gameRunning && (
            <button type="button" onClick={() => void stop()} className={`${chip} text-[12.5px] font-semibold text-ink hover:text-accent`}>
              {t('closeGame')}
            </button>
          )}
          <RebuildChip />
          <InstrumentedChip />
          <ProfileChip />
        </div>
      </div>
      <button
        type="button"
        disabled={gameRunning || installingTextures || (ready && lockedByProfile)}
        title={ready && lockedByProfile ? t(optimizer.profiling ? 'errorProfiling' : 'instrumentedDesc') : undefined}
        onClick={() => (!ready ? openWizard() : rebuilding ? setAsking(true) : void play())}
        className="gow-plaque relative mb-1 flex h-[72px] w-[300px] shrink-0 items-center justify-center gap-3 self-end font-title text-[24px] font-bold tracking-[0.14em] text-cream"
      >
        {ready ? <Play size={22} fill="currentColor" /> : <Sparkles size={22} />}
        {gameRunning
          ? t('running')
          : installingTextures
            ? t('texturePackPlayBlocked')
            : !ready
              ? t('wizInstall')
              : optimizer.profiling
                ? t('playLockedProfile')
                : optimizer.instrumented
                  ? t('playLockedBuild')
                  : t('play')}
      </button>
      {asking && (
        <PlayDuringRebuildDialog
          onWait={() => setAsking(false)}
          onPlay={() => {
            setAsking(false);
            void play();
          }}
        />
      )}
    </section>
  );
}

// ----------------------------------------------------------------------------------------------- sections

export type SectionId = 'graphics' | 'textures' | 'performance' | 'files' | 'saves' | 'controls' | 'cheats' | 'other' | 'retro';

export const sections: { id: SectionId; key: string; icon: IconComponent }[] = [
  { id: 'graphics', key: 'tabGraphics', icon: Monitor },
  { id: 'textures', key: 'tabTextures', icon: Layers },
  { id: 'performance', key: 'tabPerformance', icon: Gauge },
  { id: 'files', key: 'tabFiles', icon: FolderOpen },
  { id: 'saves', key: 'tabSaves', icon: Save },
  { id: 'controls', key: 'tabControls', icon: Gamepad2 },
  { id: 'cheats', key: 'tabCheats', icon: Sparkles },
  { id: 'other', key: 'tabOther', icon: Settings2 },
  { id: 'retro', key: 'tabRetroAchievements', icon: Trophy },
];

export function NavRail({ section, onSelect }: { section: SectionId; onSelect: (id: SectionId) => void }) {
  const { t, textureInstall } = useApp();
  // Installing the texture pack takes minutes and its progress belongs to the Textures section: nothing else is reachable.
  const locked = textureInstall.active;
  return (
    <nav className="relative flex w-[252px] shrink-0 flex-col" aria-label={t('navSections')}>
      <div className="gow-fill gow-fill-fade" />
      <div className="gow-frame" />
      <div className="relative flex flex-col gap-1 px-[14px] pt-[26px]">
        {sections.map(({ id, key, icon: Icon }) => {
          const active = id === section;
          const blocked = locked && !active;
          return (
            <button
              key={id}
              type="button"
              title={blocked ? t('texturePackLocked') : t(key)}
              aria-label={t(key)}
              aria-current={active ? 'page' : undefined}
              disabled={blocked}
              onClick={() => onSelect(id)}
              className={`relative flex h-[46px] items-center gap-3 rounded-[5px] pl-[30px] text-left font-serif text-[16px] font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
                active ? 'gow-plaque text-cream [--end:36px] [--pad:4px]' : blocked ? 'text-ink' : 'text-ink hover:bg-[#6f4a22]/15'
              }`}
            >
              <Icon size={22} strokeWidth={1.7} className={active ? 'text-cream' : 'text-ink-soft'} />
              <span className="truncate">{t(key)}</span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}

// ----------------------------------------------------------------------------------------------- overlays

export function Toast() {
  const { toast } = useApp();
  return (
    <div
      role="status"
      aria-live="polite"
      className={`pointer-events-none fixed bottom-[30px] left-1/2 z-20 -translate-x-1/2 rounded-full border-2 border-gold bg-[#241710]/95 px-[22px] py-2.5 text-sm font-semibold text-cream shadow-lg transition-opacity duration-300 ${
        toast ? 'opacity-100' : 'opacity-0'
      }`}
    >
      {toast}
    </div>
  );
}

export function Modal({ children, width = 560 }: { children: React.ReactNode; width?: number }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[#140b06]/65 p-6" role="dialog" aria-modal="true">
      <div className="gow-panel max-h-full overflow-y-auto" style={{ width: width + 60 }}>
        <div className="p-1">{children}</div>
      </div>
    </div>
  );
}

const MINUTE_MS = 60_000;
/** No processor use and no output for this long: the build may be stuck (a long silent step still uses the processor). */
const STUCK_MS = 10 * MINUTE_MS;
/** Below this share of one core the build counts as idle (the same threshold as the backend's `BUSY_PERCENT`). */
const BUSY_PERCENT = 5;

/** `Building CXX object ps2xRuntime/CMakeFiles/x.dir/runner/a.cpp.obj` -> `Building CXX object a.cpp.obj`. */
export function shortenStep(text: string): string {
  return text
    .split(' ')
    .map((word) => (word.includes('/') ? (word.replace(/,$/, '').split('/').pop() ?? word) + (word.endsWith(',') ? ',' : '') : word))
    .join(' ');
}

/** The installer's steps (`Step '...'` in install.ps1 / install.sh) the interface names in the player's language, by prefix. */
const knownSteps: Array<[string, string]> = [
  ['Checking tools', 'buildStepTools'],
  ['Downloading the compiler', 'buildStepDownloadCompiler'],
  ['Downloading portable Git', 'buildStepDownloadGit'],
  ['Downloading portable', 'buildStepDownloadTool'],
  ['Building the Rust tools', 'buildStepRustTools'],
  ['Game data', 'buildStepGameData'],
  ['Translating', 'buildStepTranslate'],
  ['VU1 microcode', 'buildStepVu1'],
  ['Configuring the build', 'buildStepConfigure'],
  ['Building the game with instrumentation', 'buildStepInstrumented'],
  ['Building the game with the speed profile', 'buildStepProfiled'],
  ['Building the game', 'buildStepBuild'],
  ['Optimization:', 'buildStepTraining'],
  ['Done', 'buildStepDone'],
];

/** The step in the player's language, or the installer's own text for one the interface does not know. */
export function stepLabel(step: string, t: (key: string) => string): string {
  const known = knownSteps.find(([prefix]) => step.startsWith(prefix));
  return known ? t(known[1]) : step;
}

export type BuildHealth = 'working' | 'downloading' | 'waiting' | 'stuck' | 'unknown';

/** Whether the build is working, waiting a moment, or may be stuck (see `BuildActivity`). */
export function buildHealth(activity: BuildActivity, now: number): BuildHealth {
  // A download waits on the connection, not the processor: a slow one is never called stuck (the first installation only).
  if (activity.step.startsWith('Downloading')) return 'downloading';
  if (activity.cpuPercent < 0) return 'unknown';
  if (activity.cpuPercent >= BUSY_PERCENT) return 'working';
  const lastBusy = activity.lastBusyMs || activity.startedAtMs;
  const lastOutput = activity.lastOutputMs || activity.startedAtMs;
  return now - lastBusy >= STUCK_MS && now - lastOutput >= STUCK_MS ? 'stuck' : 'waiting';
}

/**
 * GOW-Port (issue #32, "Rebuild transparency"): next to every build progress bar. The bar only moves when ninja finishes a
 * file, and some steps print nothing for many minutes, so it can look stuck while it works. The details say what runs,
 * whether the processor is being used (the real sign of life), how long since anything was printed, and the last lines.
 */
export function BuildDetailsButton({ className = '' }: { className?: string }) {
  const { optimizer, api } = useApp();
  if (!optimizer.compiling) return null;
  return <ActivityDetailsButton activity={optimizer.activity} onOpenLog={() => void api.openOptimizerLog()} className={className} />;
}

/** The same button for any build: the rebuilds above, and the installation wizard (with its own log). */
export function ActivityDetailsButton({ activity, onOpenLog, className = '' }: { activity: BuildActivity; onOpenLog: () => void; className?: string }) {
  const { t } = useApp();
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className={`h-[22px] shrink-0 rounded border border-line bg-transparent px-2.5 text-[11px] font-semibold tracking-wide text-ink transition-colors hover:text-accent ${className}`}
      >
        {t('buildDetails')}
      </button>
      {/* Out of the chip: a banner ancestor clips and repositions fixed elements, so the window goes to the page body. */}
      {open && createPortal(<BuildDetailsDialog activity={activity} onOpenLog={onOpenLog} onClose={() => setOpen(false)} />, document.body)}
    </>
  );
}

export function BuildDetailsDialog({ activity, onOpenLog, onClose }: { activity: BuildActivity; onOpenLog: () => void; onClose: () => void }) {
  const { lang, t, f, duration } = useApp();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const health = buildHealth(activity, now);
  const since = (ms: number) => duration(Math.max(0, Math.floor((now - ms) / 1000)));
  const cores = (activity.cpuPercent / 100).toLocaleString(lang, { maximumFractionDigits: 1 });
  const healthText =
    health === 'working'
      ? activity.cpuPercent >= 150
        ? f('buildWorkingCores', cores)
        : f('buildWorkingCore', Math.round(activity.cpuPercent))
      : health === 'stuck'
        ? f('buildStuck', since(Math.max(activity.lastBusyMs, activity.lastOutputMs) || activity.startedAtMs))
        : health === 'waiting'
          ? f('buildWaiting', since(activity.lastBusyMs || activity.startedAtMs))
          : health === 'downloading'
            ? t('buildDownloading')
            : t('buildCpuUnknown');
  const quiet = activity.lastOutputMs > 0 && now - activity.lastOutputMs >= MINUTE_MS;
  const rows: Array<[string, string]> = [];
  if (activity.step) rows.push([t('buildStep'), stepLabel(activity.step, t)]);
  if (activity.filesTotal > 0) rows.push([t('buildFiles'), f('buildFilesValue', activity.filesDone, activity.filesTotal)]);
  if (activity.startedAtMs > 0) rows.push([t('buildElapsed'), since(activity.startedAtMs)]);
  if (activity.lastOutputMs > 0) rows.push([t('buildLastOutput'), f('buildAgo', since(activity.lastOutputMs))]);
  if (activity.lastFinished) rows.push([t('buildLastFinished'), shortenStep(activity.lastFinished)]);
  const tone =
    health === 'working' ? 'border-[#3f6b2a] bg-[#3f6b2a]/10 text-[#2f5520]' : health === 'stuck' ? 'border-[#a3190f] bg-[#a3190f]/10 text-[#a3190f]' : 'border-line bg-[#4a2f18]/10 text-ink';
  return (
    <Modal width={640}>
      <div className="flex flex-col gap-3">
        <h2 className="gow-text-shadow font-title text-[18px] font-bold uppercase tracking-wide text-ink">{t('buildDetailsTitle')}</h2>
        <p role="status" aria-live="polite" className={`rounded border-2 px-3 py-2 text-[14px] font-semibold ${tone}`}>
          {healthText}
        </p>
        {rows.length > 0 && (
          <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-[13.5px]">
            {rows.map(([label, value]) => (
              <div key={label} className="contents">
                <dt className="font-semibold text-ink-soft">{label}</dt>
                <dd className="min-w-0 break-words text-ink">{value}</dd>
              </div>
            ))}
          </dl>
        )}
        {quiet && health !== 'stuck' && health !== 'downloading' && <p className="text-[13px] text-ink-soft">{t('buildQuietHint')}</p>}
        {activity.recentLines.length > 0 && (
          <div className="flex flex-col gap-1">
            <span className="text-[12px] font-semibold uppercase tracking-wide text-ink-soft">{t('buildRecentLines')}</span>
            <pre className="max-h-44 overflow-auto whitespace-pre-wrap break-all rounded border border-line bg-field/80 p-2.5 font-mono text-[11.5px] text-ink-soft">
              {activity.recentLines.join('\n')}
            </pre>
          </div>
        )}
        <div className="flex justify-end gap-2">
          <SecondaryButton onClick={onOpenLog}>{t('buildOpenLog')}</SecondaryButton>
          <PrimaryButton onClick={onClose}>{t('buildClose')}</PrimaryButton>
        </div>
      </div>
    </Modal>
  );
}

/**
 * The window was asked to close while a texture pack installs (the backend kept it open). Closing now throws the
 * download away, so the player decides.
 */
export function CloseBlockedDialog() {
  const { closeBlocked, textureInstall, optimizer, keepInstalling, closeAnyway, t } = useApp();
  if (!closeBlocked) return null;
  // GOW-Port: the backend holds the window open during "Optimize now" too: closing kills it, and its game, halfway.
  if (optimizer.profiling && !textureInstall.active) {
    return (
      <Modal>
        <div className="flex flex-col gap-3.5">
          <h2 className="gow-text-shadow font-title text-[18px] font-bold uppercase tracking-wide text-ink">{t('profileCloseTitle')}</h2>
          <p className="text-[15px] font-medium text-ink">{t('profileCloseText')}</p>
          <div className="flex justify-end gap-2">
            <SecondaryButton danger onClick={closeAnyway}>{t('texturePackCloseAnyway')}</SecondaryButton>
            <SecondaryButton onClick={keepInstalling}>{t('profileCancelKeep')}</SecondaryButton>
          </div>
        </div>
      </Modal>
    );
  }
  if (!textureInstall.active) return null;
  return (
    <Modal>
      <div className="flex flex-col gap-3.5">
        <h2 className="gow-text-shadow font-title text-[18px] font-bold uppercase tracking-wide text-ink">{t('texturePackCloseTitle')}</h2>
        <p className="text-[15px] font-medium text-ink">{t('texturePackCloseText')}</p>
        <div className="flex justify-end gap-2">
          <SecondaryButton danger onClick={closeAnyway}>{t('texturePackCloseAnyway')}</SecondaryButton>
          <SecondaryButton onClick={keepInstalling}>{t('texturePackKeep')}</SecondaryButton>
        </div>
      </div>
    </Modal>
  );
}

export function ErrorDialog() {
  const { error, dismissError, api, t } = useApp();
  const report = useReportExport();
  if (!error) return null;
  return (
    <Modal>
      <div className="flex flex-col gap-3.5">
        <p className="text-[15px] font-medium text-ink">{error.text}</p>
        {error.hint && <p className="text-[13px] text-ink-soft">{error.hint}</p>}
        {error.detail && (
          <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded border border-line bg-field/80 p-2.5 font-mono text-xs text-ink-soft">
            {error.detail}
          </pre>
        )}
        <MarkField />
        <div className="flex justify-end gap-2">
          <SecondaryButton onClick={() => void api.openLog()}>{t('viewLog')}</SecondaryButton>
          <SecondaryButton disabled={report.busy} onClick={() => void report.run()}>{t('reportExportButton')}</SecondaryButton>
          <SecondaryButton onClick={dismissError}>{t('gotIt')}</SecondaryButton>
        </div>
      </div>
    </Modal>
  );
}
