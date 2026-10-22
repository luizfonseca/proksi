import type { ComponentChildren } from 'preact';
import { useState } from 'preact/hooks';
import { niceMax } from '../../lib/format';
import { Panel } from './Page';

export type Series = { name: string; color: string; values: (number | null)[] };

const WIDTH = 1000;

/** A value on the chart's left, over its grid line */
const TICK = 'pointer-events-none absolute left-0 bg-base-100 pr-1 text-xs leading-none text-base-content/45';

/** The last `span` values, newest at the right, as an SVG path (gaps where there's no value) */
function path(values: (number | null)[], span: number, max: number, height: number): string {
  const start = span - values.length;
  let d = '';
  let drawing = false;
  values.forEach((value, index) => {
    if (value === null) {
      drawing = false;
      return;
    }
    const x = ((start + index) / Math.max(1, span - 1)) * WIDTH;
    const y = height - (Math.min(value, max) / max) * height;
    d += `${drawing ? 'L' : 'M'}${x.toFixed(1)},${y.toFixed(1)}`;
    drawing = true;
  });
  return d;
}

type Props = {
  title: string;
  series: Series[];
  /** Points shown (one a second) */
  span: number;
  format: (value: number) => string;
  height?: number;
  /** The chart's top, instead of the largest value */
  max?: number;
};

/** Lines over time, with a legend of the latest values and the values under the pointer */
export function LineChart({ title, series, span, format, height = 80, max }: Props) {
  const [hover, setHover] = useState<number | null>(null);
  const shown = series.map((one) => ({ ...one, values: one.values.slice(-span) }));
  const largest = Math.max(0, ...shown.flatMap((one) => one.values.filter((value): value is number => value !== null)));
  const top = max ?? niceMax(largest);
  const length = Math.max(0, ...shown.map((one) => one.values.length));
  // The index under the pointer, in the shown values
  const at = hover === null ? null : Math.round(hover * (span - 1)) - (span - length);
  const valueAt = (values: (number | null)[]) => {
    const index = at !== null && at >= 0 && at < values.length ? at : values.length - 1;
    return values[index] ?? null;
  };

  return (
    <Panel
      title={title}
      aside={
        <Legend>
          {shown.map((one) => {
            const value = valueAt(one.values);
            return (
              <Swatch color={one.color}>
                {one.name} <LegendValue>{value === null ? '—' : format(value)}</LegendValue>
              </Swatch>
            );
          })}
        </Legend>
      }
    >
      <div
        class="plot relative mx-4 mb-3"
        style={{ height: `${height}px` }}
        onMouseMove={(event) => {
          const box = event.currentTarget.getBoundingClientRect();
          setHover(Math.min(1, Math.max(0, (event.clientX - box.left) / box.width)));
        }}
        onMouseLeave={() => setHover(null)}
      >
        <span class={`${TICK} top-0.5`}>{format(top)}</span>
        <span class={`${TICK} top-[calc(50%+2px)]`}>{format(top / 2)}</span>
        <svg viewBox={`0 0 ${WIDTH} ${height}`} preserveAspectRatio="none" role="img" aria-label={title}>
          {/* The middle and the bottom: a line at the top would read as the header's border */}
          {[0.5, 1].map((share) => (
            <line class="grid" x1="0" x2={WIDTH} y1={height * share} y2={height * share} />
          ))}
          {shown.map((one) => (
            <path d={path(one.values, span, top, height)} stroke={one.color} />
          ))}
          {hover !== null && <line class="cursor" x1={hover * WIDTH} x2={hover * WIDTH} y1="0" y2={height} />}
        </svg>
        {length === 0 && <span class="absolute inset-0 grid place-items-center text-xs text-base-content/45">Waiting for data…</span>}
      </div>
    </Panel>
  );
}

/** What's beside a panel's title: what its lines are, and their latest values */
export function Legend({ children }: { children: ComponentChildren }) {
  return <div class="flex flex-wrap items-center gap-3 text-xs text-base-content/60">{children}</div>;
}

/** A color in a legend, and what it means */
export function Swatch({ color, children }: { color: string; children: ComponentChildren }) {
  return (
    <span class="inline-flex items-center">
      <i class="mr-1.5 inline-block size-2 rounded-xs" style={{ background: color }} />
      {children}
    </span>
  );
}

/** A value in a legend, darker than what names it */
export function LegendValue({ children }: { children: ComponentChildren }) {
  return <b class="ml-0.5 font-semibold text-base-content tabular-nums">{children}</b>;
}

/** A small line of the latest values, for a card */
export function Sparkline({
  values,
  color,
  class: placed = 'absolute inset-x-0 bottom-0 h-10',
}: {
  values: (number | null)[];
  color: string;
  /** Where it's drawn: across the bottom of a card, unless placed otherwise */
  class?: string;
}) {
  // The last 2 minutes, across the card from the first point
  const span = Math.min(120, Math.max(30, values.length));
  const shown = values.slice(-span);
  const top = niceMax(Math.max(0, ...shown.filter((value): value is number => value !== null)));
  return (
    <svg class={`sparkline w-full ${placed}`} viewBox={`0 0 ${WIDTH} 40`} preserveAspectRatio="none" aria-hidden="true">
      <path d={path(shown, span, top, 40)} stroke={color} />
    </svg>
  );
}
