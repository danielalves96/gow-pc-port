import {
  Archive, Download, FileText, Flag, Gamepad2, Info, Languages, Link, Minimize2, Rocket, RotateCcw, SkipForward, Zap,
} from 'lucide-react';
import { MarkField, useReportExport } from '../components/ReportTools';
import { SecondaryButton, Select, SettingRow, Switch } from '../components/ui';
import { useApp } from '../state/app';

export function OtherPage() {
  const { settings, patch, reset, api, info, notify, t, f } = useApp();
  const report = useReportExport();
  const shortcut = async (desktop: boolean) => {
    if (await api.createShortcut(desktop)) notify(t('shortcutDone'));
  };
  // Steam's "Add a Non-Steam Game" asks for the launcher's executable; the path is the one thing the player cannot guess.
  const copyLauncherPath = async () => {
    try {
      await navigator.clipboard.writeText(info.launcherPath);
      notify(t('steamCopied'));
    } catch {
      notify(f('steamCopyFailed', info.launcherPath));
    }
  };

  return (
    <div className="flex flex-col gap-1">
      <SettingRow icon={Languages} title={t('language')}>
        <Select
          label={t('language')}
          options={[{ text: 'Português (Brasil)', value: 'pt-BR' }, { text: 'English', value: 'en' }]}
          value={settings.language}
          onChange={(language) => patch({ language })}
        />
      </SettingRow>
      <SettingRow icon={SkipForward} title={t('skipVideos')} description={t('skipVideosDesc')}>
        <Switch label={t('skipVideos')} checked={settings.skipVideos} onChange={(skipVideos) => patch({ skipVideos })} />
      </SettingRow>
      <SettingRow icon={Zap} title={t('fastBoot')} description={t('fastBootDesc')}>
        <Switch label={t('fastBoot')} checked={settings.fastBoot} onChange={(fastBoot) => patch({ fastBoot })} />
      </SettingRow>
      <SettingRow icon={Minimize2} title={t('minimizeOnPlay')} description={t('minimizeOnPlayDesc')}>
        <Switch label={t('minimizeOnPlay')} checked={settings.minimizeOnPlay} onChange={(minimizeOnPlay) => patch({ minimizeOnPlay })} />
      </SettingRow>
      <SettingRow
        icon={Rocket}
        title={t('skipLauncher')}
        description={t(info.platform === 'windows' ? 'skipLauncherDesc' : 'skipLauncherDescLinux')}
      >
        <Switch label={t('skipLauncher')} checked={settings.skipLauncher} onChange={(skipLauncher) => patch({ skipLauncher })} />
      </SettingRow>
      <SettingRow icon={Gamepad2} title={t('steam')} description={t('steamDesc')}>
        <SecondaryButton onClick={() => void copyLauncherPath()}>{t('steamCopy')}</SecondaryButton>
      </SettingRow>
      <SettingRow icon={Download} title={t('autoUpdate')} description={t('autoUpdateDesc')}>
        <Switch label={t('autoUpdate')} checked={settings.autoUpdate} onChange={(autoUpdate) => patch({ autoUpdate })} />
      </SettingRow>
      <SettingRow icon={Link} title={t('shortcuts')} description={t('shortcutsDesc')}>
        <div className="flex gap-2">
          <SecondaryButton onClick={() => void shortcut(false)}>{t('menuShortcut')}</SecondaryButton>
          <SecondaryButton onClick={() => void shortcut(true)}>{t('desktopShortcut')}</SecondaryButton>
        </div>
      </SettingRow>
      <SettingRow icon={FileText} title={t('log')} description={t('logDesc')}>
        <SecondaryButton onClick={() => void api.openLog()}>{t('open')}</SecondaryButton>
      </SettingRow>
      <SettingRow icon={Archive} title={t('reportExportTitle')} description={t('reportExportDesc')}>
        <SecondaryButton disabled={report.busy} onClick={() => void report.run()}>{t('reportExportButton')}</SecondaryButton>
      </SettingRow>
      <SettingRow icon={Flag} title={t('reportMarkTitle')} description={t('reportMarkDesc')} controlWidth={380}>
        <MarkField />
      </SettingRow>
      <SettingRow icon={RotateCcw} title={t('reset')} description={t('resetDesc')}>
        <SecondaryButton danger onClick={() => void reset()}>{t('reset')}</SecondaryButton>
      </SettingRow>
      <SettingRow icon={Info} title={t('about')} description={t('aboutText')}>
        <span className="text-[13px] text-ink-muted">v{info.version}</span>
      </SettingRow>
    </div>
  );
}
