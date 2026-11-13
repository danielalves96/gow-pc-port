import { useEffect, useState, type ReactNode } from 'react';
import { Disc3, FileCode2, Play, Save } from 'lucide-react';
import { PathField, SecondaryButton, type IconComponent } from '../components/ui';
import { useApp } from '../state/app';

function Field({ icon: Icon, title, description, children }: { icon: IconComponent; title: string; description: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[22px_1fr] gap-x-3.5 gap-y-1">
      <Icon size={22} strokeWidth={1.6} className="mt-px text-ink-soft" />
      <div>
        <div className="gow-text-shadow text-[14.5px] font-bold leading-tight text-ink">{title}</div>
        <div className="mt-0.5 text-[13px] font-medium leading-[1.25] text-[#47321f]">{description}</div>
      </div>
      <div className="col-start-2 mt-1">{children}</div>
    </div>
  );
}

export function FilesPage() {
  const { settings, patch, status, detectPaths, openWizard, api, t, f } = useApp();
  const [memoryCard, setMemoryCard] = useState('');
  useEffect(() => {
    void api.memoryCardFile(settings.elf).then(setMemoryCard);
  }, [api, settings.elf]);

  const game = status?.game;
  const data = status?.data;
  const executable = status?.executable;
  // The disc is only needed to extract the game the first time (or to repair an extraction). Once the data is in place the
  // recompiled build does not read it anymore, so the field would only be noise.
  const needsDisc = !data?.ok;

  return (
    <div className="flex flex-col gap-4">
      <Field icon={FileCode2} title={t('elf')} description={t('elfDesc')}>
        <PathField
          path={settings.elf}
          dialogTitle={t('chooseElf')}
          filters={[{ name: 'SCUS_973.99', extensions: ['99'] }, { name: '*', extensions: ['*'] }]}
          status={
            !game?.exists ? t('notFound')
              : game.modified ? t('experimentalElf')
              : game.valid ? t('elfOk')
              : f('elfInvalid', game.crc)
          }
          severity={game?.valid ? 'success' : game?.exists ? 'warning' : 'error'}
          onChange={(elf) => patch({ elf })}
        />
      </Field>
      {needsDisc && (
        <Field icon={Disc3} title={t('iso')} description={t('isoDesc')}>
          <PathField
            path={settings.iso}
            clearable
            dialogTitle={t('chooseIso')}
            filters={[{ name: 'ISO', extensions: ['iso', 'ISO'] }, { name: '*', extensions: ['*'] }]}
            status={
              data?.chosenIsoMissing ? t('chosenIsoMissing')
                : data?.iso ? f('usedIso', data.iso)
                : data?.extracted ? t('extractedIso')
                : t('isoMissing')
            }
            severity={data?.chosenIsoMissing ? 'error' : data?.ok ? 'success' : 'error'}
            onChange={(iso) => patch({ iso })}
          />
        </Field>
      )}
      <Field icon={Play} title={t('executable')} description={t('executableDesc')}>
        <PathField
          path={settings.executable}
          dialogTitle={t('chooseExecutable')}
          status={!executable?.exists ? t('notFound') : executable.executable ? t('executableOk') : t('executableNotExe')}
          severity={executable?.executable ? 'success' : 'error'}
          onChange={(path) => patch({ executable: path })}
        />
      </Field>
      {settings.elf.length > 0 && (
        <Field icon={Save} title={t('memory')} description={t('memoryDesc')}>
          <PathField path={memoryCard} folderOnly openTarget={memoryCard.replace(/[\\/][^\\/]*$/, '')} />
        </Field>
      )}
      <div className="mt-1 flex gap-2.5 pl-9">
        <SecondaryButton onClick={() => void detectPaths(true)}>{t('detect')}</SecondaryButton>
        <SecondaryButton onClick={openWizard}>{t('wizTitle')}</SecondaryButton>
      </div>
    </div>
  );
}
