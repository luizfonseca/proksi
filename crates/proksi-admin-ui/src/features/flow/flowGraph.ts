// A host's traffic as a graph: clients → host → locations → upstreams, laid out in columns,
// each line toned by what flows through it over the last seconds

import type { Host } from '../../api';
import { availability, ms, percent, rate } from '../../lib/format';
import type { HostStats, LocationStats, Percentiles } from '../live/live';

export type Tone = 'idle' | 'ok' | 'warn' | 'bad';

/**
 * The requests going through a node, to show in the request log when it's clicked: its location's,
 * and for an upstream, those sent to any of its addresses (as written, and as resolved)
 */
export type Focus = { location: string; upstreams?: string[] };

/** What a node's card shows when it's hovered */
export type Card = {
  requests: number;
  /** Responses by status class, over the window */
  responses?: Record<string, number>;
  duration?: Percentiles;
  /** Failed (refused, timed out), a second */
  failed?: number;
  health?: 'healthy' | 'unhealthy';
};

export type FlowNode = {
  id: string;
  x: number;
  y: number;
  width: number;
  title: string;
  /** Below the title: what goes through it */
  detail: string;
  tone: Tone;
  /** Locations and upstreams: the requests through them */
  filter?: Focus;
  /** Upstreams: their part of their location's traffic, from 0 to 1 */
  share?: number;
  card: Card;
};

export type FlowEdge = { from: FlowNode; to: FlowNode; rate: number; tone: Tone };

/** `idle`: nothing went through it over the window */
export type Flow = { width: number; height: number; nodes: FlowNode[]; edges: FlowEdge[]; idle: boolean };

/** Room for each upstream (or a location without any) */
export const ROW = 64;
export const NODE_HEIGHT = 46;
/** The width laid out when none is given */
export const WIDTH = 1000;

/** The widths of the columns' nodes: what's left of the width goes between them */
const NODES = { clients: 110, host: 240, location: 240, upstream: 260 };

type Column = keyof typeof NODES;

/** Each column's place and width, for a graph `width` wide: nodes keep their size, gaps grow */
function columns(width: number): Record<Column, { x: number; width: number }> {
  const names = Object.keys(NODES) as Column[];
  const taken = names.reduce((sum, name) => sum + NODES[name], 0);
  // Narrower than the nodes and their smallest gaps: wider than the room, scrolled sideways
  const gap = Math.max(40, (width - taken) / (names.length - 1));
  let x = 0;
  const placed = {} as Record<Column, { x: number; width: number }>;
  for (const name of names) {
    placed[name] = { x, width: NODES[name] };
    x += NODES[name] + gap;
  }
  return placed;
}

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
  // Availability: the responses that weren't 5xx, shown once some were
  if (errors > 0) parts.push(`${availability(errors)} avail`);
  return parts.join(' · ');
}

/**
 * `window`: the seconds the stats are over (errors are counts in it); `width`: the room it's drawn
 * in, at its actual size
 */
export function layout(host: Host, stats: HostStats | undefined, window = 5, width = WIDTH): Flow {
  const COLUMNS = columns(width);
  const statsOf = (path: string): LocationStats | undefined =>
    stats?.locations.find((location) => location.location === path);
  const rows = host.locations.reduce((sum, location) => sum + Math.max(1, location.upstreams.length), 0);
  const height = Math.max(1, rows) * ROW;
  const middle = height / 2;

  const node = (id: string, column: Column, y: number, title: string, detail: string, tone: Tone, card: Card, more?: Partial<FlowNode>): FlowNode => ({
    id,
    x: COLUMNS[column].x,
    width: COLUMNS[column].width,
    y,
    title,
    detail,
    tone,
    card,
    ...more,
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
  const hostCard: Card = { requests, responses, duration: { p50: null, p90: null, p99 } };
  const clients = node('clients', 'clients', middle, 'Clients', requests > 0 ? rate(requests) : 'idle', hostTone, { requests });
  const hostNode = node('host', 'host', middle, host.host, summary(requests, p99, responses), hostTone, hostCard);

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
      { requests: locationRate, responses: located?.responses ?? {}, duration: located?.duration_ms ?? { p50: null, p90: null, p99: null } },
      { filter: { location: location.location } },
    );
    nodes.push(locationNode);
    edges.push({ from: hostNode, to: locationNode, rate: locationRate, tone: locationTone });

    // Each upstream's traffic: answered, and failed (connection refused, timed out), which
    // isn't in its latency
    const traffic = location.upstreams.map((upstream) => {
      const used = located?.upstreams.find((candidate) => candidate.upstream === upstream.upstream);
      const answered = used?.requests_per_second ?? 0;
      const failed = (used?.errors ?? 0) / (window || 1);
      return { used, answered, failed, rate: answered + failed };
    });
    const locationTraffic = traffic.reduce((sum, one) => sum + one.rate, 0);
    location.upstreams.forEach((upstream, index) => {
      const { used, answered, failed, rate: upstreamRate } = traffic[index] ?? { used: undefined, answered: 0, failed: 0, rate: 0 };
      const share = locationTraffic > 0 ? upstreamRate / locationTraffic : 0;
      const upstreamTone: Tone = !upstream.healthy
        ? 'bad'
        : upstreamRate <= 0
          ? 'idle'
          : failed === 0
            ? 'ok'
            : failed / upstreamRate >= 0.25
              ? 'bad'
              : 'warn';
      // Its share, when it's one of several taking traffic
      const shared = location.upstreams.length > 1 && share > 0 && share < 1 ? percent(share * 100) : '';
      const detail = !upstream.healthy
        ? 'unhealthy'
        : [summary(answered, used?.duration_ms.p99 ?? null), failed > 0 ? `${rate(failed)} failed` : '', shared]
            .filter((part) => part && part !== 'idle')
            .join(' · ') || 'idle';
      const upstreamNode = node(
        `upstream ${location.location} ${upstream.upstream}`,
        'upstream',
        (row + index + 0.5) * ROW,
        upstream.name ? `${upstream.name} → ${upstream.upstream}` : upstream.upstream,
        detail,
        upstreamTone,
        {
          requests: answered,
          duration: used?.duration_ms ?? { p50: null, p90: null, p99: null },
          failed,
          health: upstream.healthy ? 'healthy' : 'unhealthy',
        },
        {
          filter: { location: location.location, upstreams: [upstream.upstream, ...upstream.addresses.map(({ address }) => address)] },
          share,
        },
      );
      nodes.push(upstreamNode);
      edges.push({ from: locationNode, to: upstreamNode, rate: upstreamRate, tone: upstreamTone });
    });
    row += span;
  }
  return { width: COLUMNS.upstream.x + COLUMNS.upstream.width, height, nodes, edges, idle: requests <= 0 && edges.every((edge) => edge.rate <= 0) };
}
