// The overview's history: a point a second from `/api/v1/stats/stream`, kept in the page's
// memory for 15 minutes (Proksi keeps nothing more than it already does)

import { useEffect, useState } from 'preact/hooks';

/** Points kept: 15 minutes, one a second */
export const KEPT = 900;

export type Percentiles = { p50: number | null; p90: number | null; p99: number | null };

export type UpstreamStats = {
  upstream: string;
  requests_per_second: number;
  healthy: boolean;
  errors: number;
  duration_ms: Percentiles;
};
export type LocationStats = {
  location: string;
  requests_per_second: number;
  responses: Record<string, number>;
  duration_ms: Percentiles;
  blocked: Record<string, number>;
  upstreams: UpstreamStats[];
};
export type HostStats = { host: string; locations: LocationStats[] };
/** A `tcp` block's connections, named as in `/api/v1/tcp` (`tcp "db" in host "a.com"`) */
export type TcpStats = {
  block: string;
  connections_per_second: number;
  active: number;
  bytes_in_per_second: number;
  bytes_out_per_second: number;
  /** Connections that couldn't reach an upstream */
  upstream_errors: number;
  duration_ms: Percentiles;
};

/** What `/api/v1/stats` reports (the parts the overview uses) */
export type Stats = {
  window_seconds: number;
  total: { requests_per_second: number; responses: Record<string, number>; duration_ms: Percentiles };
  process: {
    cpu_percent: number;
    memory_bytes: number;
    memory_limit_bytes: number | null;
    cpu_limit: number | null;
  };
  hosts: HostStats[];
  tcp: TcpStats[];
};

/** A `tcp` block's point: its new connections a second, open ones, and bytes a second both ways */
export type TcpPoint = { connections: number; active: number; bytes: number };

/** A host's point: its requests, and the 5xx among them, a second, and its latency */
export type HostPoint = { requests: number; errors: number; p50: number | null; p99: number | null };

/** One point of the charts */
export type Point = {
  at: number;
  requests: number;
  /** Responses a second, by class (`2xx`...) */
  responses: Record<string, number>;
  /** 5xx responses, as a share of all (0 to 1) */
  errors: number;
  p50: number | null;
  p90: number | null;
  p99: number | null;
  cpu: number;
  memory: number;
  hosts: Record<string, HostPoint>;
  tcp: Record<string, TcpPoint>;
};

/** A host's numbers: its locations added up (its slowest location's latency) */
export function hostPoint(host: HostStats, window: number): HostPoint {
  const requests = host.locations.reduce((sum, location) => sum + location.requests_per_second, 0);
  const errors = host.locations.reduce((sum, location) => sum + (location.responses['5xx'] ?? 0), 0) / (window || 1);
  const worst = (read: (location: LocationStats) => number | null) =>
    host.locations.reduce<number | null>((max, location) => {
      const value = read(location);
      return value === null ? max : Math.max(max ?? 0, value);
    }, null);
  return {
    requests,
    errors,
    p50: worst((location) => location.duration_ms.p50),
    p99: worst((location) => location.duration_ms.p99),
  };
}

export function point(stats: Stats, at: number): Point {
  const window = stats.window_seconds || 1;
  const responses = Object.fromEntries(
    Object.entries(stats.total.responses).map(([status, count]) => [status, count / window]),
  );
  const answered = Object.values(stats.total.responses).reduce((sum, count) => sum + count, 0);
  return {
    at,
    requests: stats.total.requests_per_second,
    responses,
    errors: answered ? (stats.total.responses['5xx'] ?? 0) / answered : 0,
    p50: stats.total.duration_ms.p50,
    p90: stats.total.duration_ms.p90,
    p99: stats.total.duration_ms.p99,
    cpu: stats.process.cpu_percent,
    memory: stats.process.memory_bytes,
    hosts: Object.fromEntries((stats.hosts ?? []).map((host) => [host.host, hostPoint(host, window)])),
    tcp: Object.fromEntries(
      (stats.tcp ?? []).map((block) => [
        block.block,
        { connections: block.connections_per_second, active: block.active, bytes: block.bytes_in_per_second + block.bytes_out_per_second },
      ]),
    ),
  };
}

/** A host in "Hosts right now": its latest numbers, and when it last had requests */
export type ActiveHost = { host: string; point: HostPoint; lastSeen: number };

/**
 * Hosts with requests since the page opened (its history): the busiest of the latest stats first,
 * then the others, most recently seen first. They stay until the history no longer has them.
 */
export function activeHosts(stats: Stats | null, history: Point[]): ActiveHost[] {
  const now = history.length ? history[history.length - 1].at : Date.now();
  const lastSeen = new Map<string, number>();
  for (const point of history) {
    for (const [host, numbers] of Object.entries(point.hosts)) {
      if (numbers.requests > 0) lastSeen.set(host, point.at);
    }
  }
  const latest = new Map((stats?.hosts ?? []).map((host) => [host.host, hostPoint(host, stats?.window_seconds ?? 1)]));
  for (const [host, point] of latest) {
    if (point.requests > 0) lastSeen.set(host, now);
  }
  const idle: HostPoint = { requests: 0, errors: 0, p50: null, p99: null };
  return [...lastSeen]
    .map(([host, seen]) => ({ host, point: latest.get(host) ?? idle, lastSeen: seen }))
    .sort((a, b) => b.point.requests - a.point.requests || b.lastSeen - a.lastSeen);
}

/** `points` with `next`, without the ones beyond `KEPT` */
export function keep(points: Point[], next: Point): Point[] {
  const kept = points.length >= KEPT ? points.slice(points.length - KEPT + 1) : points.slice();
  kept.push(next);
  return kept;
}

// Shared by every mount of the overview: the history survives moving between pages
let history: Point[] = [];
let latest: Stats | null = null;
let source: EventSource | null = null;
const listeners = new Set<() => void>();

function connect() {
  if (source) return;
  // A 5-second window smooths the lines; a point still comes every second
  source = new EventSource('/api/v1/stats/stream?window=5');
  source.onmessage = (event) => {
    latest = JSON.parse(event.data) as Stats;
    history = keep(history, point(latest, Date.now()));
    listeners.forEach((listener) => listener());
  };
}

/** The history and the latest stats, updated every second */
export function useLive(): { history: Point[]; latest: Stats | null } {
  const [, render] = useState(0);
  useEffect(() => {
    connect();
    const listener = () => render((count) => count + 1);
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }, []);
  return { history, latest };
}
