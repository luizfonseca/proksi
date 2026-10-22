import { describe, expect, it } from 'vitest';
import { easeOut } from './motion';

describe('motion', () => {
  it('eases out: quick at first, slow at the end, within its bounds', () => {
    expect(easeOut(0)).toBe(0);
    expect(easeOut(1)).toBe(1);
    expect(easeOut(0.5)).toBeCloseTo(0.875);
    expect(easeOut(-1)).toBe(0);
    expect(easeOut(2)).toBe(1);
  });
});
