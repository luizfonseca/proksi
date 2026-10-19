import { describe, expect, it } from 'vitest';
import type { Host } from './api';
import { layout, ROW, tone } from './flow';
import type { HostStats } from './live';

const host: Host = {
  host: 'shop.test',
  source: 'file',
  tls: true,
  listen: [],
  rules: [],
  locations: [
    {
      location: '/',
      blocks: [],
      rules: [],
      upstreams: [
        { upstream: '10.0.0.1:80', name: null, weight: 1, healthy: true, addresses: [] },
        { upstream: '10.0.0.2:80', name: null, weight: 1, healthy: false, addresses: [] },
        { upstream: '10.0.0.3:80', name: null, weight: 1, healthy: true, addresses: [] },
      ],
    },
    { location: '/status', blocks: ['respond'], rules: [], upstreams: [] },
  ],
};

const stats: HostStats = {
  host: 'shop.test',
  locations: [
    {
      location: '/',
      requests_per_second: 10,
      responses: { '2xx': 45, '5xx': 5 },
      duration_ms: { p50: 1, p90: 2, p99: 30 },
      blocked: {},
      upstreams: [
        { upstream: '10.0.0.1:80', requests_per_second: 10, healthy: true, errors: 0, duration_ms: { p50: 1, p90: 2, p99: 28 } },
        { upstream: '10.0.0.3:80', requests_per_second: 0, healthy: true, errors: 10, duration_ms: { p50: null, p90: null, p99: null } },
      ],
    },
  ],
};

describe('the traffic graph', () => {
  it('tones lines by their errors', () => {
    expect(tone(0, {})).toBe('idle');
    expect(tone(5, { '2xx': 10 })).toBe('ok');
    expect(tone(5, { '2xx': 9, '5xx': 1 })).toBe('warn');
    expect(tone(5, { '2xx': 3, '5xx': 1 })).toBe('bad');
  });

  it('lays out clients, the host, locations and upstreams', () => {
    const flow = layout(host, stats);

    // Three rows for the first location's upstreams, one for the second location
    expect(flow.height).toBe(4 * ROW);
    expect(flow.nodes.map((node) => node.id)).toEqual([
      'clients',
      'host',
      'location /',
      'upstream / 10.0.0.1:80',
      'upstream / 10.0.0.2:80',
      'upstream / 10.0.0.3:80',
      'location /status',
    ]);
    const edge = (to: string) => flow.edges.find((edge) => edge.to.id === to)!;
    expect(edge('host')).toMatchObject({ rate: 10, tone: 'warn' });
    expect(edge('upstream / 10.0.0.1:80')).toMatchObject({ rate: 10, tone: 'ok' });
    expect(edge('upstream / 10.0.0.2:80')).toMatchObject({ rate: 0, tone: 'bad' });
    expect(edge('location /status')).toMatchObject({ rate: 0, tone: 'idle' });
    // Refused connections aren't answered, but they're traffic, and failing
    expect(edge('upstream / 10.0.0.3:80')).toMatchObject({ rate: 2, tone: 'bad' });
    expect(flow.nodes.find((node) => node.id === 'upstream / 10.0.0.3:80')!.detail).toBe('2/s failed');
    expect(flow.nodes.find((node) => node.id === 'location /')!.detail).toBe('10/s · p99 30 ms · 10% 5xx');
  });

  it('is idle without stats', () => {
    const flow = layout(host, undefined);
    expect(flow.edges.every((edge) => edge.rate === 0)).toBe(true);
    expect(flow.nodes[0].detail).toBe('idle');
  });
});
