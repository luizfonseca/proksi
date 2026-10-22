import { describe, expect, it } from 'vitest';
import type { Host } from '../../api';
import { layout, ROW, tone } from './flowGraph';
import type { HostStats } from '../live/live';

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

  it('fills the width it has between its columns, keeping nodes their size', () => {
    const at = (flow: ReturnType<typeof layout>, id: string) => flow.nodes.find((node) => node.id === id)!;
    const wide = layout(host, stats, 5, 1280);
    expect(wide.width).toBe(1280);
    expect(at(wide, 'host').width).toBe(at(layout(host, stats), 'host').width);
    expect(at(wide, 'host').x - (at(wide, 'clients').x + at(wide, 'clients').width)).toBeCloseTo(
      at(wide, 'location /').x - (at(wide, 'host').x + at(wide, 'host').width),
    );
    // Too narrow for its nodes: as narrow as they allow, to scroll sideways
    const narrow = layout(host, stats, 5, 400);
    expect(narrow.width).toBe(110 + 240 + 240 + 260 + 3 * 40);
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
    expect(flow.nodes.find((node) => node.id === 'upstream / 10.0.0.3:80')!.detail).toBe('2.0/s failed · 17%');
    expect(flow.nodes.find((node) => node.id === 'location /')!.detail).toBe('10.0/s · p99 30 ms · 10% 5xx');
  });

  it('is idle without stats', () => {
    const flow = layout(host, undefined);
    expect(flow.idle).toBe(true);
    expect(flow.edges.every((edge) => edge.rate === 0)).toBe(true);
    expect(flow.nodes[0].detail).toBe('idle');
  });

  it("gives each upstream its share of its location's traffic", () => {
    const flow = layout(host, stats);
    const node = (id: string) => flow.nodes.find((candidate) => candidate.id === id)!;
    expect(flow.idle).toBe(false);
    // 10/s answered by the first, 2/s refused by the third
    expect(node('upstream / 10.0.0.1:80').share).toBeCloseTo(10 / 12);
    expect(node('upstream / 10.0.0.1:80').detail).toBe('10.0/s · p99 28 ms · 83%');
    expect(node('upstream / 10.0.0.2:80').share).toBe(0);
  });

  it('says what to filter requests by, and what to show when hovered', () => {
    const flow = layout(host, stats);
    const node = (id: string) => flow.nodes.find((candidate) => candidate.id === id)!;
    expect(node('host').filter).toBeUndefined();
    expect(node('location /').filter).toEqual({ location: '/' });
    expect(node('upstream / 10.0.0.1:80').filter).toEqual({ location: '/', upstreams: ['10.0.0.1:80'] });
    expect(node('location /').card).toEqual({
      requests: 10,
      responses: { '2xx': 45, '5xx': 5 },
      duration: { p50: 1, p90: 2, p99: 30 },
    });
    expect(node('upstream / 10.0.0.2:80').card).toMatchObject({ health: 'unhealthy' });
    expect(node('upstream / 10.0.0.3:80').card).toMatchObject({ failed: 2, health: 'healthy' });
  });
});
