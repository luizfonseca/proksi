import type { ComponentChildren } from 'preact';
import { PANEL } from './Page';

/** Columns of the cards: on phones, then wider */
const COLUMNS = {
  4: 'grid-cols-2 md:grid-cols-4',
  5: 'grid-cols-2 md:grid-cols-3 lg:grid-cols-5',
};

/** Numbers side by side, in one box */
export function Stats({ columns, children }: { columns: keyof typeof COLUMNS; children: ComponentChildren }) {
  return <div class={`${PANEL} stats mb-5 grid w-full grid-flow-row ${COLUMNS[columns]}`}>{children}</div>;
}

const TONES = { error: 'text-error', none: '' };

/** One number: what it counts, its value, and maybe more under it */
export function Stat({
  title,
  value,
  tone = 'none',
  detail,
  class: extra = '',
  children,
}: {
  title: ComponentChildren;
  value: ComponentChildren;
  tone?: keyof typeof TONES;
  detail?: ComponentChildren;
  class?: string;
  children?: ComponentChildren;
}) {
  return (
    <div class={`stat ${extra}`}>
      <div class="stat-title">{title}</div>
      <div class={`stat-value text-2xl tabular-nums ${TONES[tone]}`}>{value}</div>
      {detail && <div class="stat-desc">{detail}</div>}
      {children}
    </div>
  );
}

const BARS = { error: 'progress-error', warning: 'progress-warning', success: 'progress-success' };

/** How full something is: `value` of `max`, colored by how it's doing */
export function Meter({ value, max, tone }: { value: number; max: number; tone: keyof typeof BARS }) {
  return <progress class={`progress ${BARS[tone]} h-1.5 flex-1`} value={value} max={max} />;
}
