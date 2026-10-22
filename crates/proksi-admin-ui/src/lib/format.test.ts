import { describe, expect, it } from 'vitest';
import { bytes, ms, niceMax, percent, rate, relative } from './format';

describe('format', () => {
  it('reads sizes, durations, rates and shares', () => {
    expect(bytes(512)).toBe('512 B');
    expect(bytes(48 * 1024 * 1024)).toBe('48.0 MB');
    expect(bytes(null)).toBe('—');
    expect(ms(3.25)).toBe('3.3 ms');
    expect(ms(1500)).toBe('1.5 s');
    expect(rate(0.4)).toBe('0.4/s');
    expect(rate(2500)).toBe('2.5k/s');
    expect(percent(12.34)).toBe('12%');
    expect(percent(0.25)).toBe('0.3%');
  });

  it('rounds chart tops up', () => {
    expect(niceMax(0)).toBe(1);
    expect(niceMax(7)).toBe(10);
    expect(niceMax(130)).toBe(200);
    expect(niceMax(2.1)).toBe(2.5);
  });

  it('says how long ago something was, in words', () => {
    const now = Date.UTC(2026, 9, 22, 12, 0, 0);
    const ago = (seconds: number) => relative(now - seconds * 1000, now);
    expect(ago(3)).toBe('just now');
    // A clock a little ahead is still now
    expect(ago(-5)).toBe('just now');
    expect(ago(45)).toBe('45 s ago');
    expect(ago(3 * 60 + 20)).toBe('3 min ago');
    expect(ago(2 * 3600 + 100)).toBe('2 h ago');
    expect(ago(30 * 3600)).toBe('yesterday');
    expect(ago(3 * 86400)).toBe('3 days ago');
    expect(ago(40 * 86400)).toBe(new Date(now - 40 * 86400 * 1000).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }));
  });
});
