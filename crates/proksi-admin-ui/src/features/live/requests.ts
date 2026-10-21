// The latest requests, from `/api/v1/requests/stream`: watched only while asked for, since Proksi
// builds them only while someone watches. What was seen stays in the page's memory: leaving the
// page stops watching, and coming back shows it again.

import { useEffect, useRef, useState } from 'preact/hooks';

export type Served = {
  time: number;
  method: string;
  host: string;
  path: string;
  status: number;
  duration_ms: number;
  client_ip: string;
  location: string | null;
  upstream: string | null;
};

/** Requests kept on the page, newest first */
export const KEPT = 1000;

export function add(requests: Served[], served: Served): Served[] {
  return [served, ...requests.slice(0, KEPT - 1)];
}

/** `arrived` (oldest first) on top of `requests`, newest first, up to `KEPT` */
export function addAll(requests: Served[], arrived: Served[]): Served[] {
  return [...arrived.slice(-KEPT).reverse(), ...requests].slice(0, KEPT);
}

/**
 * Whether `served` has every word of `filter`, in its host and path, method, status (`503` or
 * `5xx`), client or upstream
 */
export function matches(served: Served, filter: string): boolean {
  const text = [
    `${served.host}${served.path}`,
    served.method,
    served.status,
    `${Math.floor(served.status / 100)}xx`,
    served.client_ip,
    served.upstream ?? '',
  ]
    .join(' ')
    .toLowerCase();
  return filter
    .toLowerCase()
    .split(/\s+/)
    .every((word) => text.includes(word));
}

/** What each list saw (of every host, or of one), while the page is open */
const seen = new Map<string, Served[]>();

/** How often the list changes: fast enough to feel live, slow enough to read */
const REFRESH_MS = 1000;

/**
 * The latest requests. `paused` stops watching; `held` (the pointer is on the list) keeps the
 * list still, and what arrives meanwhile is shown when it's let go.
 */
export function useRequests(paused: boolean, held: boolean, host?: string): { requests: Served[]; skipped: number } {
  const key = host ?? '';
  const [requests, setRequests] = useState<Served[]>(() => seen.get(key) ?? []);
  const [skipped, setSkipped] = useState(0);
  const arrived = useRef<Served[]>([]);
  const holding = useRef(held);
  holding.current = held;

  useEffect(() => {
    if (paused) return;
    const source = new EventSource(`/api/v1/requests/stream${host ? `?host=${encodeURIComponent(host)}` : ''}`);
    source.onmessage = (event) => {
      arrived.current.push(JSON.parse(event.data) as Served);
      if (arrived.current.length > KEPT) arrived.current.splice(0, arrived.current.length - KEPT);
    };
    source.addEventListener('skipped', (event) => setSkipped(Number((event as MessageEvent).data)));
    const timer = setInterval(() => {
      if (holding.current || arrived.current.length === 0) return;
      const batch = arrived.current;
      arrived.current = [];
      setRequests((current) => {
        const next = addAll(current, batch);
        seen.set(key, next);
        return next;
      });
    }, REFRESH_MS);
    return () => {
      source.close();
      clearInterval(timer);
    };
  }, [paused, host, key]);
  return { requests, skipped };
}
