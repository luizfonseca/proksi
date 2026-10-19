import { describe, expect, it } from 'vitest';
import { KEPT, keep, point, type Point, type Stats } from './live';

const stats: Stats = {
  window_seconds: 5,
  total: { requests_per_second: 4, responses: { '2xx': 18, '5xx': 2 }, duration_ms: { p50: 3, p90: 9, p99: 40 } },
  process: { cpu_percent: 12.5, memory_bytes: 2048, allocator_bytes: 1024, memory_limit_bytes: null, cpu_limit: null },
};

describe('the live history', () => {
  it('reads a point from the stats', () => {
    expect(point(stats, 1000)).toEqual({
      at: 1000,
      requests: 4,
      responses: { '2xx': 3.6, '5xx': 0.4 },
      errors: 0.1,
      p50: 3,
      p90: 9,
      p99: 40,
      cpu: 12.5,
      memory: 2048,
      allocator: 1024,
    });
  });

  it('has no errors without requests', () => {
    expect(point({ ...stats, total: { ...stats.total, responses: {} } }, 0).errors).toBe(0);
  });

  it('keeps 15 minutes', () => {
    let points: Point[] = [];
    for (let at = 0; at < KEPT + 10; at++) points = keep(points, point(stats, at));
    expect(points).toHaveLength(KEPT);
    expect(points[0].at).toBe(10);
    expect(points[KEPT - 1].at).toBe(KEPT + 9);
  });
});
