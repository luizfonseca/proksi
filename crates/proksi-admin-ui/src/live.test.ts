import { describe, expect, it } from 'vitest';
import { activeHosts, KEPT, keep, point, type Point, type Stats } from './live';

const stats: Stats = {
  window_seconds: 5,
  total: { requests_per_second: 4, responses: { '2xx': 18, '5xx': 2 }, duration_ms: { p50: 3, p90: 9, p99: 40 } },
  process: { cpu_percent: 12.5, memory_bytes: 2048, allocator_bytes: 1024, memory_limit_bytes: null, cpu_limit: null },
  hosts: [],
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
      hosts: {},
    });
  });

  it("adds up a host's locations", () => {
    const location = (rate: number, errors: number, p99: number | null) => ({
      location: '/',
      requests_per_second: rate,
      responses: { '2xx': rate * 5 - errors, '5xx': errors },
      duration_ms: { p50: 1, p90: 2, p99 },
      blocked: {},
      upstreams: [],
    });
    const hosts = [{ host: 'a.test', locations: [location(3, 5, 40), location(1, 0, null)] }];
    expect(point({ ...stats, hosts }, 0).hosts['a.test']).toEqual({ requests: 4, errors: 1, p50: 1, p99: 40 });
  });

  it('lists the hosts with requests, busiest first', () => {
    const host = (name: string, rate: number) => ({
      host: name,
      locations: [
        { location: '/', requests_per_second: rate, responses: { '2xx': rate * 5 }, duration_ms: { p50: 1, p90: 1, p99: 1 }, blocked: {}, upstreams: [] },
      ],
    });
    const hosts = activeHosts({ ...stats, hosts: [host('quiet.test', 0), host('a.test', 1), host('b.test', 9)] });
    expect(hosts.map(({ host }) => host)).toEqual(['b.test', 'a.test']);
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
