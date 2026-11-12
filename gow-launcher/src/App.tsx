import { useState } from 'react';
import {
  CloseBlockedDialog, ErrorDialog, Hero, NavRail, ProfileConfirmDialog, ProfileOverlay, sections, TitleBar, Toast, type SectionId,
} from './components/Shell';
import { upper } from './lib/i18n';
import { CheatsPage } from './pages/CheatsPage';
import { ControlsPage } from './pages/ControlsPage';
import { FilesPage } from './pages/FilesPage';
import { SavesPage } from './pages/SavesPage';
import { GraphicsPage } from './pages/GraphicsPage';
import { OtherPage } from './pages/OtherPage';
import { PerformancePage } from './pages/PerformancePage';
import { TexturesPage } from './pages/TexturesPage';
import { RetroAchievementsPage } from './pages/RetroAchievementsPage';
import { AppProvider, useApp } from './state/app';
import { Wizard } from './wizard/Wizard';

const pages: Record<SectionId, () => React.JSX.Element> = {
  graphics: GraphicsPage,
  textures: TexturesPage,
  performance: PerformancePage,
  files: FilesPage,
  saves: SavesPage,
  controls: ControlsPage,
  cheats: CheatsPage,
  other: OtherPage,
  retro: RetroAchievementsPage,
};

/**
 * GOW-Port: the parchment of the dialogs (`panel.webp`) and the red button (`button-red.webp`) are only used inside dialogs,
 * so the window fetched and decoded them the first time a dialog opened, and that dialog was drawn for a while without its
 * parchment and with a bare red button (the "Optimize now" confirmation made it the first thing many players open). Painted
 * once here, tiny, almost transparent and under the title bar, they are ready before any dialog opens.
 */
function PrewarmDialogImages() {
  return (
    <div aria-hidden="true" className="pointer-events-none absolute left-0 top-0 flex opacity-[0.01]">
      <div className="gow-panel h-[64px] w-[64px]" />
      <div className="gow-button-red h-[24px] w-[24px]" />
    </div>
  );
}

function Panel() {
  const { t, wizardOpen, lang } = useApp();
  const [section, setSection] = useState<SectionId>('graphics');
  const current = sections.find((entry) => entry.id === section)!;
  const Page = pages[section];
  const Icon = current.icon;
  return (
    <div className="gow-window flex h-full flex-col">
      <PrewarmDialogImages />
      <TitleBar />
      <main className="flex min-h-0 flex-1 flex-col">
        <Hero />
        <div className="flex min-h-0 flex-1 gap-2.5 px-2.5 pb-2.5">
          <NavRail section={section} onSelect={setSection} />
          <section className="relative flex min-h-0 min-w-0 flex-1 flex-col">
            <div className="gow-fill" />
            <div className="gow-frame" />
            <div className="relative flex min-h-0 flex-1 flex-col gap-2 pb-[24px] pl-[34px] pr-[19px] pt-[34px]">
              {/* Same right margin as the rows: 15px = the scrollbar gutter (9px) plus the 6px of padding. */}
              <header className="flex items-center gap-3 pr-[15px]">
                <Icon size={30} strokeWidth={1.6} className="text-ink" />
                <h1 className="gow-text-shadow font-title text-[28px] font-bold leading-none tracking-wide text-ink uppercase">{t(current.key)}</h1>
                <span className="flex-1" />
                <span className="text-[10.5px] font-semibold tracking-[0.16em] text-ink-soft">{upper(lang, t('settingsSubtitle'))}</span>
              </header>
              <div className="gow-rule mr-[15px] shrink-0" />
              <div className="min-h-0 flex-1 overflow-y-auto pr-1.5 [scrollbar-gutter:stable]">
                <Page />
              </div>
            </div>
          </section>
        </div>
      </main>
      <Toast />
      <ProfileOverlay />
      <ProfileConfirmDialog />
      <ErrorDialog />
      <CloseBlockedDialog />
      {wizardOpen && <Wizard />}
    </div>
  );
}

export default function App() {
  return (
    <AppProvider>
      <Panel />
    </AppProvider>
  );
}
