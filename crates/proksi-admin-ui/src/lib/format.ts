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

export function rate(value: number): string {
  if (value >= 1000) return `${(value / 1000).toFixed(1)}k/s`;
  return `${value >= 100 ? Math.round(value) : Number(value.toFixed(1))}/s`;
}

export function percent(value: number): string {
  return `${value >= 10 ? Math.round(value) : Number(value.toFixed(1))}%`;
}

/**
 * Availability, from the share of responses that were 5xx (0 to 1): what wasn't. Closer to 100%,
 * with more decimals, and rounded down, so errors never show as 100%.
 */
export function availability(errors: number): string {
  if (errors <= 0) return '100%';
  const value = (1 - Math.min(1, errors)) * 100;
  const decimals = value >= 99 ? 2 : value >= 90 ? 1 : 0;
  const scale = 10 ** decimals;
  // A hair over, so 99.6 isn't read as 99.59999…
  return `${Number((Math.floor(value * scale + 1e-9) / scale).toFixed(decimals))}%`;
}

/** The smallest round number at least `value`: the top of a chart */
export function niceMax(value: number): number {
  if (!(value > 0)) return 1;
  const magnitude = 10 ** Math.floor(Math.log10(value));
  const step = [1, 2, 2.5, 5, 10].find((step) => step * magnitude >= value) ?? 10;
  return step * magnitude;
}
