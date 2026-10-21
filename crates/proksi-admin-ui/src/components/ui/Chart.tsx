import { useState } from 'preact/hooks';
import { niceMax } from '../../lib/format';

export type Series = { name: string; color: string; values: (number | null)[] };

const WIDTH = 1000;

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
export function LineChart({ title, series, span, format, height = 140, max }: Props) {
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
    <div class="panel chart">
      <div class="chart-head">
        <h3>{title}</h3>
        <div class="legend">
          {shown.map((one) => {
            const value = valueAt(one.values);
            return (
              <span>
                <i style={{ background: one.color }} />
                {one.name} <b>{value === null ? '—' : format(value)}</b>
              </span>
            );
          })}
        </div>
      </div>
      <div
        class="plot"
        style={{ height }}
        onMouseMove={(event) => {
          const box = event.currentTarget.getBoundingClientRect();
          setHover(Math.min(1, Math.max(0, (event.clientX - box.left) / box.width)));
        }}
        onMouseLeave={() => setHover(null)}
      >
        <span class="tick top">{format(top)}</span>
        <span class="tick middle">{format(top / 2)}</span>
        <svg viewBox={`0 0 ${WIDTH} ${height}`} preserveAspectRatio="none" role="img" aria-label={title}>
          {[0, 0.5, 1].map((share) => (
            <line class="grid" x1="0" x2={WIDTH} y1={height * share} y2={height * share} />
          ))}
          {shown.map((one) => (
            <path d={path(one.values, span, top, height)} stroke={one.color} />
          ))}
          {hover !== null && <line class="cursor" x1={hover * WIDTH} x2={hover * WIDTH} y1="0" y2={height} />}
        </svg>
        {length === 0 && <span class="waiting">Waiting for data…</span>}
      </div>
    </div>
  );
}

/** A small line of the latest values, for a card */
export function Sparkline({ values, color }: { values: (number | null)[]; color: string }) {
  // The last 2 minutes, across the card from the first point
  const span = Math.min(120, Math.max(30, values.length));
  const shown = values.slice(-span);
  const top = niceMax(Math.max(0, ...shown.filter((value): value is number => value !== null)));
  return (
    <svg class="sparkline" viewBox={`0 0 ${WIDTH} 40`} preserveAspectRatio="none" aria-hidden="true">
      <path d={path(shown, span, top, 40)} stroke={color} />
    </svg>
  );
}
