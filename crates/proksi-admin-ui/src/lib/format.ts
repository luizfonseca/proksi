// Numbers as the overview shows them

export function bytes(value: number | null): string {
  if (value === null) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value >= 100 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

export function ms(value: number | null): string {
  if (value === null) return '—';
  if (value >= 1000) return `${(value / 1000).toFixed(value >= 10000 ? 0 : 1)} s`;
  return `${value >= 100 ? Math.round(value) : Number(value.toFixed(1))} ms`;
}

/** Requests (or connections) a second: one decimal under 100/s, even a 0, so the width holds as it changes */
export function rate(value: number): string {
  if (value >= 1000) return `${(value / 1000).toFixed(1)}k/s`;
  const tenths = Math.round(value * 10) / 10;
  return `${tenths >= 100 ? Math.round(value) : tenths.toFixed(1)}/s`;
}

export function percent(value: number): string {
  return `${value >= 10 ? Math.round(value) : Number(value.toFixed(1))}%`;
}

/** How long before `now` the time `at` was (both in milliseconds), in words: "3 min ago" */
export function relative(at: number, now: number): string {
  const seconds = Math.floor((now - at) / 1000);
  if (seconds < 10) return 'just now';
  if (seconds < 60) return `${seconds} s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  const days = Math.floor(seconds / 86400);
  if (days === 1) return 'yesterday';
  if (days < 30) return `${days} days ago`;
  return new Date(at).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/** The smallest round number at least `value`: the top of a chart */
export function niceMax(value: number): number {
  if (!(value > 0)) return 1;
  const magnitude = 10 ** Math.floor(Math.log10(value));
  const step = [1, 2, 2.5, 5, 10].find((step) => step * magnitude >= value) ?? 10;
  return step * magnitude;
}
