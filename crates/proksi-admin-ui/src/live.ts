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

/** What `/api/v1/stats` reports (the parts the overview uses) */
export type Stats = {
  window_seconds: number;
  total: { requests_per_second: number; responses: Record<string, number>; duration_ms: Percentiles };
  process: {
    cpu_percent: number;
    memory_bytes: number;
    allocator_bytes: number;
    memory_limit_bytes: number | null;
    cpu_limit: number | null;
  };
  hosts: HostStats[];
};

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
  allocator: number;
  hosts: Record<string, HostPoint>;
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
    allocator: stats.process.allocator_bytes,
    hosts: Object.fromEntries((stats.hosts ?? []).map((host) => [host.host, hostPoint(host, window)])),
  };
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
