// A host's traffic as a graph: clients → host → locations → upstreams, laid out in columns,
// each line toned by what flows through it over the last seconds

import type { Host } from '../../api';
import { ms, percent, rate } from '../../lib/format';
import type { HostStats, LocationStats } from '../live/live';

export type Tone = 'idle' | 'ok' | 'warn' | 'bad';

export type FlowNode = {
  id: string;
  x: number;
  y: number;
  width: number;
  title: string;
  /** Below the title: what goes through it */
  detail: string;
  tone: Tone;
};

export type FlowEdge = { from: FlowNode; to: FlowNode; rate: number; tone: Tone };

export type Flow = { width: number; height: number; nodes: FlowNode[]; edges: FlowEdge[] };

/** Room for each upstream (or a location without any) */
export const ROW = 64;
export const NODE_HEIGHT = 46;
export const WIDTH = 1000;

const COLUMNS = {
  clients: { x: 0, width: 110 },
  host: { x: 160, width: 240 },
  location: { x: 450, width: 240 },
  upstream: { x: 740, width: 260 },
};

/** How a scope's responses went: errors (5xx) as a share of all, and nothing when idle */
export function tone(requests: number, responses: Record<string, number>): Tone {
  if (requests <= 0) return 'idle';
  const all = Object.values(responses).reduce((sum, count) => sum + count, 0);
  const errors = responses['5xx'] ?? 0;
  if (!all || errors === 0) return 'ok';
  return errors / all >= 0.25 ? 'bad' : 'warn';
}

function errorShare(responses: Record<string, number>): number {
  const all = Object.values(responses).reduce((sum, count) => sum + count, 0);
  return all ? (responses['5xx'] ?? 0) / all : 0;
}

function summary(requests: number, p99: number | null, responses?: Record<string, number>): string {
  if (requests <= 0) return 'idle';
  const parts = [rate(requests)];
  if (p99 !== null) parts.push(`p99 ${ms(p99)}`);
  const errors = responses ? errorShare(responses) : 0;
  if (errors > 0) parts.push(`${percent(errors * 100)} 5xx`);
  return parts.join(' · ');
}

/** `window`: the seconds the stats are over (errors are counts in it) */
export function layout(host: Host, stats: HostStats | undefined, window = 5): Flow {
  const statsOf = (path: string): LocationStats | undefined =>
    stats?.locations.find((location) => location.location === path);
  const rows = host.locations.reduce((sum, location) => sum + Math.max(1, location.upstreams.length), 0);
  const height = Math.max(1, rows) * ROW;
  const middle = height / 2;

  const node = (id: string, column: keyof typeof COLUMNS, y: number, title: string, detail: string, tone: Tone): FlowNode => ({
    id,
    x: COLUMNS[column].x,
    width: COLUMNS[column].width,
    y,
    title,
    detail,
    tone,
  });

  // The host: its locations added up
  const requests = stats?.locations.reduce((sum, location) => sum + location.requests_per_second, 0) ?? 0;
  const responses: Record<string, number> = {};
  for (const location of stats?.locations ?? []) {
    for (const [status, count] of Object.entries(location.responses)) responses[status] = (responses[status] ?? 0) + count;
  }
  const p99 = (stats?.locations ?? []).reduce<number | null>(
    (max, location) => (location.duration_ms.p99 === null ? max : Math.max(max ?? 0, location.duration_ms.p99)),
    null,
  );
  const hostTone = tone(requests, responses);
  const clients = node('clients', 'clients', middle, 'Clients', requests > 0 ? rate(requests) : 'idle', hostTone);
  const hostNode = node('host', 'host', middle, host.host, summary(requests, p99, responses), hostTone);

  const nodes = [clients, hostNode];
  const edges: FlowEdge[] = [{ from: clients, to: hostNode, rate: requests, tone: hostTone }];

  let row = 0;
  for (const location of host.locations) {
    const span = Math.max(1, location.upstreams.length);
    const located = statsOf(location.location);
    const locationRate = located?.requests_per_second ?? 0;
    const locationTone = tone(locationRate, located?.responses ?? {});
    const locationNode = node(
      `location ${location.location}`,
      'location',
      (row + span / 2) * ROW,
      location.location,
      summary(locationRate, located?.duration_ms.p99 ?? null, located?.responses),
      locationTone,
    );
    nodes.push(locationNode);
    edges.push({ from: hostNode, to: locationNode, rate: locationRate, tone: locationTone });

    location.upstreams.forEach((upstream, index) => {
      const used = located?.upstreams.find((candidate) => candidate.upstream === upstream.upstream);
      // Answered, and failed (connection refused, timed out): failures aren't in its latency
      const answered = used?.requests_per_second ?? 0;
      const failed = (used?.errors ?? 0) / (window || 1);
      const upstreamRate = answered + failed;
      const upstreamTone: Tone = !upstream.healthy
        ? 'bad'
        : upstreamRate <= 0
          ? 'idle'
          : failed === 0
            ? 'ok'
            : failed / upstreamRate >= 0.25
              ? 'bad'
              : 'warn';
      const detail = !upstream.healthy
        ? 'unhealthy'
        : [summary(answered, used?.duration_ms.p99 ?? null), failed > 0 ? `${rate(failed)} failed` : '']
            .filter((part) => part && part !== 'idle')
            .join(' · ') || 'idle';
      const upstreamNode = node(
        `upstream ${location.location} ${upstream.upstream}`,
        'upstream',
        (row + index + 0.5) * ROW,
        upstream.name ? `${upstream.name} → ${upstream.upstream}` : upstream.upstream,
        detail,
        upstreamTone,
      );
      nodes.push(upstreamNode);
      edges.push({ from: locationNode, to: upstreamNode, rate: upstreamRate, tone: upstreamTone });
    });
    row += span;
  }
  return { width: WIDTH, height, nodes, edges };
}
