import { useEffect, useId, useRef, useState, type ButtonHTMLAttributes, type ComponentType, type ReactNode } from 'react';
import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Check, ChevronDown, Circle, Delete, Diamond, Lock, Minus, Square, Triangle, X } from 'lucide-react';
import { useApp } from '../state/app';

const cx = (...parts: Array<string | false | undefined | null>) => parts.filter(Boolean).join(' ');

// ----------------------------------------------------------------------------------------------- glyphs

const GLYPHS: Record<string, IconComponent> = {
  '✕': X, '○': Circle, '□': Square, '△': Triangle, '↑': ArrowUp, '↓': ArrowDown, '←': ArrowLeft, '→': ArrowRight, '⌫': Delete, '—': Minus,
};
const GLYPH_SPLIT = new RegExp(`([${Object.keys(GLYPHS).join('')}])`);

/** Draws the pad and arrow symbols of a text as icons (the original character stays for screen readers). */
export function Glyphs({ children, size = 14 }: { children: string; size?: number }) {
  if (!GLYPH_SPLIT.test(children)) return <>{children}</>;
  return (
    <span className="inline-flex items-center gap-[3px]">
      {children.split(GLYPH_SPLIT).map((part, index) => {
        const Icon = GLYPHS[part];
        if (!Icon) return part ? <span key={index}>{part}</span> : null;
        return (
          <span key={index} className="inline-flex">
            <Icon size={size} strokeWidth={2.6} className="shrink-0" />
            <span className="sr-only">{part}</span>
          </span>
        );
      })}
    </span>
  );
}

// ----------------------------------------------------------------------------------------------- buttons

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  danger?: boolean;
}

/** Parchment button with a bronze rim; the dangerous one is blood red. */
export function SecondaryButton({ danger, className, children, ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      {...rest}
      className={cx(
        'h-[38px] text-[13px] font-semibold tracking-wide',
        danger ? 'gow-button gow-button-danger' : 'gow-button gow-text-shadow text-ink',
        'disabled:cursor-default disabled:opacity-40',
        className,
      )}
    >
      {children}
    </button>
  );
}

/** Blood red, for the action that advances the flow. */
export function PrimaryButton({ className, children, ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      {...rest}
      className={cx(
        'gow-button-red h-11 min-w-[132px] px-3 font-title text-sm font-bold tracking-[0.09em] text-cream',
        'disabled:cursor-default disabled:opacity-45',
        className,
      )}
    >
      {children}
    </button>
  );
}

// ----------------------------------------------------------------------------------------------- groups

/** Heading that opens a group of cards: a gem, the title in small caps and a rule that fades out. */
export function GroupTitle({ children }: { children: ReactNode }) {
  return (
    <div className="mb-1 mt-5 flex items-center gap-2.5 px-1 first:mt-0">
      <Diamond size={11} fill="currentColor" className="shrink-0 text-accent" />
      <h2 className="gow-text-shadow text-[12.5px] font-bold uppercase tracking-[0.2em] text-gold-dark">{children}</h2>
      <span className="h-px flex-1 bg-gradient-to-r from-line to-transparent" />
    </div>
  );
}

// ----------------------------------------------------------------------------------------------- rows

export type IconComponent = ComponentType<{ size?: number; className?: string; strokeWidth?: number }>;

interface SettingRowProps {
  title: string;
  description?: string;
  icon?: IconComponent;
  disabled?: boolean;
  hidden?: boolean;
  controlWidth?: number;
  children?: ReactNode;
}

/** Icon, title and description on the left, the control on the right. Each setting is a strip of parchment. */
export function SettingRow({ title, description, icon, disabled, hidden, controlWidth = 270, children }: SettingRowProps) {
  if (hidden) return null;
  const Icon = disabled ? Lock : (icon ?? Diamond);
  return (
    <div
      className={cx('gow-row relative flex min-h-[64px] shrink-0 items-center gap-3.5 py-2.5 pl-[22px] pr-[18px]', )}
      aria-disabled={disabled}
    >
      <Icon size={22} strokeWidth={1.6} className="shrink-0 text-ink-soft" />
      <div className="min-w-0 flex-1">
        <div className="gow-text-shadow text-[14.5px] font-bold leading-tight text-ink">
          <Glyphs>{title}</Glyphs>
        </div>
        {description && <div className="mt-0.5 text-[13px] font-medium leading-[1.25] text-[#47321f]">{description}</div>}
      </div>
      {children !== undefined && (
        <div className="flex shrink-0 justify-end" style={{ width: controlWidth }}>
          {children}
        </div>
      )}
    </div>
  );
}

/** A callout with a bar on the left. */
export function Note({ title, children, tone = 'gold' }: { title?: string; children: ReactNode; tone?: 'gold' | 'warn' }) {
  return (
    <div className={cx('relative rounded-[5px] border bg-field/80 py-3.5 pl-6 pr-4', tone === 'warn' ? 'border-warn' : 'border-line')}>
      <div className={cx('absolute left-2 top-2 bottom-2 w-[3px] rounded-sm', tone === 'warn' ? 'bg-warn' : 'bg-accent')} />
      {title && <div className="mb-1 font-title text-sm font-bold text-gold-light">{title}</div>}
      <div className={cx('text-[13px] leading-[1.15]', tone === 'warn' ? 'text-warn' : 'text-ink-soft')}>{children}</div>
    </div>
  );
}

/** A thin progress bar; with no known fraction (`undefined`, or below zero) it pulses instead of filling. */
export function ProgressBar({ label, fraction, className }: { label: string; fraction?: number; className?: string }) {
  const known = typeof fraction === 'number' && fraction >= 0;
  const percent = known ? Math.min(100, Math.round(fraction * 100)) : 0;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={known ? percent : undefined}
      className={cx('h-[6px] overflow-hidden rounded-full bg-ink/20', className)}
    >
      <div className={cx('h-full rounded-full bg-accent', !known && 'w-1/3 animate-pulse')} style={known ? { width: `${percent}%` } : undefined} />
    </div>
  );
}

// ----------------------------------------------------------------------------------------------- switch

export function Switch({
  checked, onChange, label, disabled,
}: { checked: boolean; onChange: (value: boolean) => void; label?: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className="gow-switch h-[27px] w-[58px] shrink-0 disabled:cursor-not-allowed disabled:opacity-40"
    />
  );
}

// ----------------------------------------------------------------------------------------------- slider

export function Slider({
  value, from = 0, to = 1, step = 0.01, onChange, label,
}: { value: number; from?: number; to?: number; step?: number; onChange: (value: number) => void; label?: string }) {
  const fraction = (value - from) / (to - from);
  return (
    <div className="flex w-full items-center gap-3">
      <div className="relative h-[30px] flex-1">
        <div className="gow-track absolute left-0 right-0 top-1/2 h-[12px] -translate-y-1/2">
          <div className="gow-track-fill h-full" style={{ width: `${Math.max(0, Math.min(1, fraction)) * 100}%` }} />
        </div>
        <input
          type="range"
          aria-label={label}
          min={from}
          max={to}
          step={step}
          value={value}
          onChange={(event) => onChange(Number(event.target.value))}
          className="absolute inset-0 h-full w-full cursor-pointer"
        />
      </div>
      <span className="w-11 text-right text-[13px] font-semibold text-gold-light">{Math.round(fraction * 100)}%</span>
    </div>
  );
}

// ----------------------------------------------------------------------------------------------- select

export interface Option<V extends string | number | boolean = string> {
  text: string;
  value: V;
  detail?: string;
  disabled?: boolean;
}

/** Dropdown list in the launcher's style. */
export function Select<V extends string | number | boolean>({
  options, value, onChange, disabled, label,
}: { options: Option<V>[]; value: V; onChange: (value: V) => void; disabled?: boolean; label?: string }) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const root = useRef<HTMLDivElement>(null);
  const listId = useId();
  const current = options.findIndex((option) => option.value === value);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [open]);

  const choose = (index: number) => {
    const option = options[index];
    if (!option || option.disabled) return;
    setOpen(false);
    if (option.value !== value) onChange(option.value);
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'Escape') return setOpen(false);
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!open) {
        setActive(current);
        return setOpen(true);
      }
      const step = event.key === 'ArrowDown' ? 1 : -1;
      let next = active;
      do next = (next + step + options.length) % options.length;
      while (options[next]?.disabled && next !== active);
      setActive(next);
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      if (open && active >= 0) choose(active);
      else {
        setActive(current);
        setOpen(true);
      }
    }
  };

  return (
    <div ref={root} className={cx('relative w-full', disabled && 'pointer-events-none opacity-50 grayscale')}>
      <button
        type="button"
        role="combobox"
        aria-label={label}
        aria-expanded={open}
        aria-controls={listId}
        disabled={disabled}
        onClick={() => {
          setActive(current);
          setOpen(!open);
        }}
        onKeyDown={onKeyDown}
        className="gow-select flex h-[38px] w-full items-center justify-between gap-2 text-left text-[13.5px] font-semibold text-ink"
      >
        <span className="truncate">{current >= 0 ? <Glyphs>{options[current].text}</Glyphs> : ''}</span>
        <ChevronDown size={16} strokeWidth={2.5} className={cx('shrink-0 text-ink-soft transition-transform duration-150', open && 'rotate-180 text-accent')} />
      </button>
      {open && (
        <ul
          id={listId}
          role="listbox"
          className="gow-list absolute left-0 right-0 top-[calc(100%+4px)] z-40 max-h-[340px] overflow-y-auto p-1"
        >
          {options.map((option, index) => (
            <li
              key={String(option.value)}
              role="option"
              aria-selected={index === current}
              aria-disabled={option.disabled}
              onMouseEnter={() => setActive(index)}
              onClick={() => choose(index)}
              className={cx(
                'relative cursor-pointer rounded-[3px] px-3 py-1.5',
                option.disabled && 'cursor-default opacity-45',
                index === current && 'gow-option-selected',
                !option.disabled && index === active && 'gow-option-active',
              )}
            >
              {index === current && <span className={cx('absolute left-0 top-1.5 bottom-1.5 w-[3px] rounded-sm', index === active ? 'bg-cream' : 'bg-accent')} />}
              <div className={cx('truncate text-[13.5px]', index === current && 'font-bold', index === current && index !== active && 'text-gold-light')}>
                <Glyphs>{option.text}</Glyphs>
              </div>
              {option.detail && <div className={cx('truncate text-xs', index === active ? 'text-cream/90' : 'text-ink-muted')}>{option.detail}</div>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

// ----------------------------------------------------------------------------------------------- inputs

export function TextInput({ className, ...rest }: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...rest}
      className={cx(
        'h-10 w-full rounded-[7px] border-2 border-line bg-[#e4d3b2]/90 px-2.5 text-[13px] font-semibold text-ink shadow-[inset_0_1px_3px_rgba(60,35,10,0.35)] placeholder:text-ink-muted focus:border-accent focus:outline-none',
        className,
      )}
    />
  );
}

export type Severity = 'success' | 'warning' | 'error';

const severityDot: Record<Severity, string> = { success: 'bg-ok', warning: 'bg-warn', error: 'bg-bad' };
const severityText: Record<Severity, string> = { success: 'text-ink-soft', warning: 'text-warn', error: 'text-bad' };

export function StatusLine({ severity, children }: { severity: Severity; children: ReactNode }) {
  return (
    <div className="flex items-center gap-[7px]">
      <span className={cx('h-2 w-2 shrink-0 rounded-full', severityDot[severity])} />
      <span className={cx('truncate text-[13px]', severityText[severity])}>{children}</span>
    </div>
  );
}

interface PathFieldProps {
  path: string;
  status?: string;
  severity?: Severity;
  dialogTitle?: string;
  filters?: { name: string; extensions: string[] }[];
  clearable?: boolean;
  folderOnly?: boolean;
  /** What "Open" opens when it is not `path` itself (e.g. the folder of a file that may not exist yet). */
  openTarget?: string;
  onChange?: (path: string) => void;
}

/** Editable path with "Browse" and a status line. */
export function PathField({ path, status, severity = 'success', dialogTitle, filters, clearable, folderOnly, openTarget, onChange }: PathFieldProps) {
  const { api, t } = useApp();
  const [draft, setDraft] = useState(path);
  useEffect(() => setDraft(path), [path]);
  const commit = () => {
    if (draft.trim() !== path) onChange?.(draft.trim());
  };
  const browse = async () => {
    const chosen = await api.pick({ title: dialogTitle ?? '', filters, defaultPath: path || undefined });
    if (chosen) onChange?.(chosen);
  };
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-2">
        <TextInput
          value={draft}
          readOnly={folderOnly}
          placeholder="—"
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => event.key === 'Enter' && commit()}
        />
        {!folderOnly && <SecondaryButton onClick={() => void browse()}>{t('browse')}</SecondaryButton>}
        {clearable && path.length > 0 && <SecondaryButton onClick={() => onChange?.('')}>{t('empty')}</SecondaryButton>}
        {folderOnly && <SecondaryButton onClick={() => void api.openDirectory(openTarget ?? path)}>{t('open')}</SecondaryButton>}
      </div>
      {status && <StatusLine severity={severity}>{status}</StatusLine>}
    </div>
  );
}

// ----------------------------------------------------------------------------------------------- keycap

export function KeyCap({ children }: { children: ReactNode }) {
  return (
    <span className="relative inline-flex h-7 min-w-[30px] items-center justify-center rounded border border-line-light bg-gradient-to-b from-[#f0e0bf] to-[#cdb58a] px-2 pb-[3px] text-xs font-bold text-ink">
      {children}
      <span className="absolute inset-x-px bottom-px h-[3px] rounded-[3px] bg-[#6f5233]" />
    </span>
  );
}

/** Round step marker used by the wizard. */
export function CheckDot({ done, current, size = 20 }: { done?: boolean; current?: boolean; size?: number }) {
  return (
    <span
      className={cx(
        'flex shrink-0 items-center justify-center rounded-full border',
        done ? 'border-gold-dark bg-gold' : current ? 'border-accent' : 'border-line',
      )}
      style={{ width: size, height: size }}
    >
      {done ? <Check size={size * 0.55} className="text-cream" strokeWidth={3} /> : current ? <span className="h-2 w-2 animate-pulse-soft rounded-full bg-accent" /> : null}
    </span>
  );
}
