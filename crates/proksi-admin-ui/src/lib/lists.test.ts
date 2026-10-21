import { describe, expect, it } from 'vitest';
import { asList } from './lists';

describe('asList', () => {
  it('reads one value or a list as a list, as Proksi accepts both', () => {
    expect(asList(['/a', '/b'])).toEqual(['/a', '/b']);
    expect(asList('/v2')).toEqual(['/v2']);
    expect(asList(3000)).toEqual(['3000']);
    expect(asList(undefined)).toEqual([]);
    expect(asList(null)).toEqual([]);
    expect(asList('')).toEqual([]);
  });
});
