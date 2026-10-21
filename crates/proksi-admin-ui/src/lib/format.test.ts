import { describe, expect, it } from 'vitest';
import { bytes, ms, niceMax, percent, rate } from './format';

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
});
