import { useCallback, useEffect, useState } from 'react';
import { History, Play, Save, Undo2 } from 'lucide-react';
import { GroupTitle, Note, SecondaryButton, SettingRow } from '../components/ui';
import { useApp } from '../state/app';
import type { CardSlot, SaveArt, SaveBackup } from '../lib/types';

const noArt: SaveArt = { places: {}, difficulties: [], thumbnails: {} };

/** `h:mm:ss`, as the game shows the play time. */
export function playTime(seconds: number): string {
  const total = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${Math.floor(total / 3600)}:${pad(Math.floor(total / 60) % 60)}:${pad(total % 60)}`;
}

/** The place of a save as the game names it, or its zone code when the disc could not be read. */
function placeOf(slot: CardSlot, art: SaveArt, f: (key: string, ...args: (string | number)[]) => string) {
  return art.places[String(slot.savePoint)] ?? f('savesZone', slot.zone || '?');
}

/** What the page shows of a save in a backup: its place on the card, where it is and when the game wrote it. */
function SlotLine({ slot, locale, art }: { slot: CardSlot; locale: string; art: SaveArt }) {
  const { f } = useApp();
  return (
    <div className="flex flex-wrap items-baseline gap-x-3 text-[13px] font-medium text-[#47321f]">
      <span className="font-bold text-ink">{f('savesSlot', slot.index + 1)}</span>
      <span>{placeOf(slot, art, f)}</span>
      <span>{playTime(slot.playSeconds)}</span>
      <span>{slot.modifiedUnix > 0 ? new Date(slot.modifiedUnix * 1000).toLocaleString(locale) : ''}</span>
    </div>
  );
}

/** A save of the card as the game's own list shows it: thumbnail, place, difficulty and play time, and a way to load it. */
function SlotCard({ slot, art, locale, onLoad }: { slot: CardSlot; art: SaveArt; locale: string; onLoad: (slot: CardSlot) => void }) {
  const { t, f } = useApp();
  const thumbnail = art.thumbnails[String(slot.savePoint)];
  const place = placeOf(slot, art, f);
  const difficulty = art.difficulties[slot.difficulty] ?? f('savesDifficulty', slot.difficulty + 1);
  return (
    <div className="flex items-center gap-4 rounded-md border border-[#b08a55] bg-[#f3e4c3]/60 p-2.5" data-testid="save-slot">
      {thumbnail ? (
        <img src={thumbnail} alt={place} width={128} height={80} className="h-[80px] w-[128px] shrink-0 rounded-sm border border-[#5f4326] object-cover" />
      ) : (
        <div aria-hidden className="h-[80px] w-[128px] shrink-0 rounded-sm border border-dashed border-[#b08a55]" />
      )}
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="truncate text-[15px] font-bold text-ink">{place}</div>
        <div className="text-[13px] font-semibold text-[#3d2b1b]">{`${difficulty} · ${playTime(slot.playSeconds)}`}</div>
        <div className="text-[12px] font-medium text-[#47321f]">
          {f('savesSlot', slot.index + 1)}
          {slot.modifiedUnix > 0 ? ` · ${new Date(slot.modifiedUnix * 1000).toLocaleString(locale)}` : ''}
        </div>
      </div>
      <SecondaryButton onClick={() => onLoad(slot)} title={t('savesLoadHint')}>
        <span className="inline-flex items-center gap-1.5"><Play size={14} />{t('savesLoad')}</span>
      </SecondaryButton>
    </div>
  );
}

export function SavesPage() {
  const { settings, api, t, f, gameRunning, lang, play } = useApp();
  const locale = lang === 'pt-BR' ? 'pt-BR' : 'en';
  const [slots, setSlots] = useState<CardSlot[]>([]);
  const [art, setArt] = useState<SaveArt>(noArt);
  const [loading, setLoading] = useState<CardSlot | null>(null);
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

  // The names and thumbnails come from the installed disc: read again only when the card shows another altar.
  const points = [...new Set(slots.map((slot) => slot.savePoint).filter((point) => point > 0))].sort((a, b) => a - b).join(',');
  useEffect(() => {
    let current = true;
    void api.saveArt(points ? points.split(',').map(Number) : []).then((next) => {
      if (current) setArt(next);
    });
    return () => {
      current = false;
    };
  }, [api, points, settings.elf, settings.iso]);

  // While the game runs a load throws away the progress since the last altar: it asks first. With the game closed the
  // request waits for the next start, and the game opens on that slot.
  const load = async (slot: CardSlot) => {
    setLoading(null);
    try {
      const running = await api.loadSaveSlot(settings.elf, slot.index);
      setMessage({ text: f(running ? 'savesLoadAsked' : 'savesLoadStarting', slot.index + 1), warn: false });
      if (!running) await play();
    } catch (error) {
      setMessage({ text: f('savesLoadFailed', String(error)), warn: true });
    }
  };
  const askLoad = (slot: CardSlot) => (gameRunning ? setLoading(slot) : void load(slot));

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
        <div className="flex flex-col gap-2">
          {slots.map((slot) => <SlotCard key={slot.index} slot={slot} art={art} locale={locale} onLoad={askLoad} />)}
        </div>
      )}
      {loading && (
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[13px] font-semibold text-ink">{f('savesLoadConfirm', loading.index + 1)}</span>
          <SecondaryButton danger onClick={() => void load(loading)}>{t('savesLoadYes')}</SecondaryButton>
          <SecondaryButton onClick={() => setLoading(null)}>{t('savesRestoreCancel')}</SecondaryButton>
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
              {openSlots.length === 0 ? <div className="text-[13px] font-medium text-[#47321f]">{t('savesCardEmpty')}</div> : openSlots.map((slot) => <SlotLine key={slot.index} slot={slot} locale={locale} art={art} />)}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
