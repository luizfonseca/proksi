import type { ComponentChildren, InputHTMLAttributes, JSX, SelectHTMLAttributes, TextareaHTMLAttributes } from 'preact';
import { useState } from 'preact/hooks';
import { Chip } from './Badge';
import { Search } from './icons';

/** The width given in `class`, or `otherwise` */
function width(classes: string, otherwise: string) {
  return /(^|\s)(max-)?w-/.test(classes) ? '' : otherwise;
}

/** A setting: its name above, maybe a hint, and its input */
export function Field({
  label,
  hint,
  class: extra = '',
  children,
}: {
  label: ComponentChildren;
  hint?: ComponentChildren;
  class?: string;
  children: ComponentChildren;
}) {
  return (
    <label class={`grid gap-1.5 text-xs font-medium text-base-content/60 ${extra}`}>
      <span>
        {label}
        {hint && <span class="ml-1 font-normal text-base-content/45">{hint}</span>}
      </span>
      {children}
    </label>
  );
}

type InputProps = { mono?: boolean; class?: string; type?: 'text' | 'number' | 'password' | 'url' } & Omit<
  InputHTMLAttributes<HTMLInputElement>,
  'class' | 'role' | 'type'
>;

/** A text input across its space (`mono` for addresses, paths, names) */
export function Input({ mono, class: extra = '', type = 'text', ...rest }: InputProps) {
  // Each input type has its own attributes in Preact's types: these are for text-like ones
  const props = { ...rest, type, class: `input ${width(extra, 'w-full')} ${mono ? 'font-mono' : ''} ${extra}` } as JSX.IntrinsicElements['input'];
  return <input {...props} />;
}

/** A dropdown, as wide as its options */
export function Select({ class: extra = '', children, ...rest }: { class?: string; children: ComponentChildren } & Omit<SelectHTMLAttributes<HTMLSelectElement>, 'class' | 'role'>) {
  return (
    <select class={`select ${width(extra, 'w-auto')} ${extra}`} {...rest}>
      {children}
    </select>
  );
}

/** Text over several lines (`mono` for JSON) */
export function TextArea({ mono, class: extra = '', ...rest }: { mono?: boolean; class?: string } & Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, 'class'>) {
  return <textarea class={`textarea w-full ${mono ? 'font-mono text-xs' : ''} ${extra}`} {...rest} />;
}

/** A box to tick, with what it does and maybe a hint */
export function Checkbox({
  label,
  hidden,
  hint,
  neutral,
  checked,
  onChange,
  class: extra = '',
}: {
  label: ComponentChildren;
  /** The label is only for screen readers (a box in a table row) */
  hidden?: boolean;
  hint?: ComponentChildren;
  /** Stays grey when ticked (a choice among others, rather than turning something on) */
  neutral?: boolean;
  checked: boolean;
  onChange: (checked: boolean) => void;
  class?: string;
}) {
  return (
    <label class={`flex cursor-pointer items-center gap-2 text-sm ${extra}`}>
      <input
        class={`checkbox checkbox-sm ${neutral ? '' : 'checked:checkbox-primary'}`}
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.currentTarget.checked)}
      />
      <span class={hidden ? 'sr-only' : ''}>
        {label}
        {hint && <small class="ml-2 text-xs text-base-content/45">{hint}</small>}
      </span>
    </label>
  );
}

/** A search box, with its icon */
export function SearchInput({
  placeholder,
  value,
  onInput,
  size = 'sm',
  class: extra = '',
}: {
  placeholder: string;
  value: string;
  onInput: (value: string) => void;
  size?: 'xs' | 'sm';
  class?: string;
}) {
  return (
    <label class={`input ${size === 'xs' ? 'input-xs' : 'input-sm'} min-w-0 ${extra}`}>
      <Search class="size-4 text-base-content/45" />
      <input class="grow" placeholder={placeholder} value={value} onInput={(e) => onInput(e.currentTarget.value)} />
    </label>
  );
}

/** One of a few choices, side by side */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
}: {
  options: readonly T[] | { value: T; label: ComponentChildren }[];
  value: T;
  onChange: (value: T) => void;
  label?: string;
}) {
  const choices = options.map((option) => (typeof option === 'string' ? { value: option, label: option } : option));
  return (
    <div role="tablist" aria-label={label} class="tabs tabs-box tabs-xs shrink-0 border border-base-300">
      {choices.map((choice) => (
        <button type="button" role="tab" class={`tab ${choice.value === value ? 'tab-active' : ''}`} aria-pressed={choice.value === value} onClick={() => onChange(choice.value)}>
          {choice.label}
        </button>
      ))}
    </div>
  );
}

/** A list of words: typed, then added with Enter or a comma */
export function ListInput({ value, onChange, placeholder }: { value: string[]; onChange: (value: string[]) => void; placeholder?: string }) {
  const [draft, setDraft] = useState('');
  const commit = () => {
    const words = draft.split(',').map((word) => word.trim()).filter(Boolean);
    if (words.length) onChange([...value, ...words]);
    setDraft('');
  };
  return (
    <div class="input h-auto min-h-(--size) w-full flex-wrap gap-1 px-1 py-1">
      {value.map((word, index) => (
        <Chip>
          {word}
          <button type="button" class="ml-1 cursor-pointer text-xs text-base-content/45 hover:text-base-content" aria-label={`Remove ${word}`} onClick={() => onChange(value.filter((_, at) => at !== index))}>
            ✕
          </button>
        </Chip>
      ))}
      <input
        class="min-w-20 flex-1 px-1"
        value={draft}
        placeholder={value.length ? '' : placeholder ?? 'Type, then Enter'}
        onInput={(e) => setDraft(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ',') {
            e.preventDefault();
            commit();
          } else if (e.key === 'Backspace' && !draft && value.length) {
            onChange(value.slice(0, -1));
          }
        }}
        onBlur={commit}
      />
    </div>
  );
}
