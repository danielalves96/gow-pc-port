import { useCallback, useEffect, useState } from 'react';
import { History, Save, Undo2 } from 'lucide-react';
import { GroupTitle, Note, SecondaryButton, SettingRow } from '../components/ui';
import { useApp } from '../state/app';
import type { CardSlot, SaveBackup } from '../lib/types';

/** What the page shows of a save: its place on the card and when the game wrote it. */
function SlotLine({ slot, locale }: { slot: CardSlot; locale: string }) {
  const { f } = useApp();
  return (
    <div className="flex flex-wrap items-baseline gap-x-3 text-[13px] font-medium text-[#47321f]">
      <span className="font-bold text-ink">{f('savesSlot', slot.index + 1)}</span>
      <span>{f('savesZone', slot.zone || '?')}</span>
      <span>{slot.modifiedUnix > 0 ? new Date(slot.modifiedUnix * 1000).toLocaleString(locale) : ''}</span>
    </div>
  );
}

export function SavesPage() {
  const { settings, api, t, f, gameRunning, lang } = useApp();
  const locale = lang === 'pt-BR' ? 'pt-BR' : 'en';
  const [slots, setSlots] = useState<CardSlot[]>([]);
  const [backups, setBackups] = useState<SaveBackup[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [openSlots, setOpenSlots] = useState<CardSlot[]>([]);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [message, setMessage] = useState<{ text: string; warn: boolean } | null>(null);

  const refresh = useCallback(async () => {
    const [nextSlots, nextBackups] = await Promise.all([api.cardSlots(settings.elf), api.saveBackups(settings.elf)]);
    setSlots(nextSlots);
    setBackups(nextBackups);
  }, [api, settings.elf]);

  // The game adds a copy at every altar while it runs: the page follows it only while it is being looked at.
  useEffect(() => {
    void refresh();
    if (!gameRunning) return undefined;
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => window.clearInterval(timer);
  }, [refresh, gameRunning]);

  const sourceText = (source: string) =>
    source === 'auto' ? t('savesSourceAuto') : source === 'before-restore' ? t('savesSourceBeforeRestore') : t('savesSourceManual');

  const toggle = async (id: string) => {
    if (open === id) {
      setOpen(null);
      return;
    }
    setOpenSlots(await api.saveBackupSlots(settings.elf, id));
    setOpen(id);
  };

  const backupNow = async () => {
    try {
      await api.backupCardNow(settings.elf);
      setMessage(null);
      await refresh();
    } catch (error) {
      setMessage({ text: f('savesBackupFailed', String(error)), warn: true });
    }
  };

  const restore = async (id: string) => {
    setConfirming(null);
    try {
      await api.restoreSaveBackup(settings.elf, id);
      setMessage({ text: t('savesRestoreDone'), warn: false });
      await refresh();
    } catch (error) {
      setMessage({ text: f('savesRestoreFailed', String(error)), warn: true });
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <Note>{t('savesIntro')}</Note>
      {message && <Note tone={message.warn ? 'warn' : 'gold'}>{message.text}</Note>}
      <GroupTitle>{t('savesCardTitle')}</GroupTitle>
      {slots.length === 0 ? (
        <div className="text-[13px] font-medium text-[#47321f]">{t('savesCardEmpty')}</div>
      ) : (
        <div className="flex flex-col gap-1.5">
          {slots.map((slot) => <SlotLine key={slot.index} slot={slot} locale={locale} />)}
        </div>
      )}
      <GroupTitle>{t('savesHistoryTitle')}</GroupTitle>
      <SettingRow icon={Save} title={t('savesBackupNow')} description="" controlWidth={200}>
        <SecondaryButton onClick={() => void backupNow()} disabled={settings.elf.length === 0}>{t('savesBackupNow')}</SecondaryButton>
      </SettingRow>
      {backups.length === 0 && <div className="text-[13px] font-medium text-[#47321f]">{t('savesHistoryEmpty')}</div>}
      {gameRunning && backups.length > 0 && <Note tone="warn">{t('savesRestoreGameRunning')}</Note>}
      {backups.map((backup) => (
        <div key={backup.id} className="flex flex-col gap-1.5">
          <SettingRow icon={History} title={new Date(backup.createdUnix * 1000).toLocaleString(locale)} description={sourceText(backup.source)} controlWidth={330}>
            <div className="flex items-center gap-2">
              <SecondaryButton onClick={() => void toggle(backup.id)}>{open === backup.id ? t('savesHide') : t('savesShow')}</SecondaryButton>
              {confirming === backup.id ? (
                <>
                  <SecondaryButton danger onClick={() => void restore(backup.id)}>{t('savesRestoreYes')}</SecondaryButton>
                  <SecondaryButton onClick={() => setConfirming(null)}>{t('savesRestoreCancel')}</SecondaryButton>
                </>
              ) : (
                <SecondaryButton disabled={gameRunning} title={gameRunning ? t('savesRestoreGameRunning') : t('savesRestoreConfirm')} onClick={() => setConfirming(backup.id)}>
                  <span className="inline-flex items-center gap-1.5"><Undo2 size={14} />{t('savesRestore')}</span>
                </SecondaryButton>
              )}
            </div>
          </SettingRow>
          {confirming === backup.id && <div className="pl-9 text-[13px] font-semibold text-ink">{t('savesRestoreConfirm')}</div>}
          {open === backup.id && (
            <div className="flex flex-col gap-1 pl-9">
              {openSlots.length === 0 ? <div className="text-[13px] font-medium text-[#47321f]">{t('savesCardEmpty')}</div> : openSlots.map((slot) => <SlotLine key={slot.index} slot={slot} locale={locale} />)}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
