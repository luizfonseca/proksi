import { describe, expect, it } from 'vitest';
import type { HistoryEntry } from '../../api';
import { activeVersion, canonical, diffLines } from './versions';

describe('canonical', () => {
  it('sorts keys at every level, so only real differences show', () => {
    expect(canonical({ b: 1, a: { d: [2, { f: 1, e: 0 }], c: null } })).toBe(
      canonical({ a: { c: null, d: [2, { e: 0, f: 1 }] }, b: 1 }),
    );
    expect(canonical({ b: 1, a: 2 })).toBe('{\n  "a": 2,\n  "b": 1\n}');
  });
});

describe('diffLines', () => {
  it('marks removed and added lines, keeping the others', () => {
    expect(diffLines('a\nb\nc', 'a\nx\nc\nd')).toEqual([
      { kind: 'same', text: 'a' },
      { kind: 'removed', text: 'b' },
      { kind: 'added', text: 'x' },
      { kind: 'same', text: 'c' },
      { kind: 'added', text: 'd' },
    ]);
  });

  it('has nothing to show for equal texts', () => {
    expect(diffLines('a\nb', 'a\nb').every((line) => line.kind === 'same')).toBe(true);
  });
});

describe('activeVersion', () => {
  const entry = (revision: number, change: HistoryEntry): HistoryEntry => ({ ...change, revision });
  const saved = { change: 'saved', at: 0, author: null } as HistoryEntry;

  it('is the newest change: a save, a rollback, or none after a deletion', () => {
    expect(activeVersion([entry(3, saved), entry(2, saved)])).toBe(3);
    expect(activeVersion([{ revision: 4, at: 0, author: null, change: 'rolled_back', to: 2 }, entry(3, saved)])).toBe(2);
    expect(activeVersion([{ revision: 5, at: 0, author: null, change: 'deleted' }, entry(3, saved)])).toBe(null);
    expect(activeVersion([])).toBe(null);
  });
});
