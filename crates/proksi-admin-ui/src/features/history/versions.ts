import type { HistoryEntry } from '../../api';

// Comparing versions of a host, and reading its history

/** JSON with sorted keys and two-space indents: equal definitions read the same */
export function canonical(value: unknown): string {
  return JSON.stringify(sorted(value), null, 2);
}

function sorted(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sorted);
  if (value === null || typeof value !== 'object') return value;
  const entries = Object.entries(value as Record<string, unknown>).sort(([a], [b]) => a.localeCompare(b));
  return Object.fromEntries(entries.map(([key, item]) => [key, sorted(item)]));
}

export type DiffLine = { kind: 'same' | 'added' | 'removed'; text: string };

/** The lines of `after` against `before`: the longest common subsequence is kept, removed
 * lines come before the lines that replace them */
export function diffLines(before: string, after: string): DiffLine[] {
  const a = before.split('\n');
  const b = after.split('\n');
  // common[i][j]: the longest common subsequence of a[i..] and b[j..]
  const common = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      common[i][j] = a[i] === b[j] ? common[i + 1][j + 1] + 1 : Math.max(common[i + 1][j], common[i][j + 1]);
    }
  }
  const lines: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      lines.push({ kind: 'same', text: a[i++] });
      j++;
    } else if (j >= b.length || (i < a.length && common[i + 1][j] >= common[i][j + 1])) {
      lines.push({ kind: 'removed', text: a[i++] });
    } else {
      lines.push({ kind: 'added', text: b[j++] });
    }
  }
  return lines;
}

/** The version in use after these changes (newest first), or null if the host was deleted */
export function activeVersion(changes: HistoryEntry[]): number | null {
  const [last] = changes;
  if (!last || last.change === 'deleted') return null;
  return last.change === 'rolled_back' ? last.to : last.revision;
}
