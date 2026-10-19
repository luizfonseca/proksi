// The latest requests, from `/api/v1/requests/stream`: watched only while the overview is open,
// since Proksi builds them only while someone watches

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
export const SHOWN = 100;

export function add(requests: Served[], served: Served): Served[] {
  return [served, ...requests.slice(0, SHOWN - 1)];
}

/** `arrived` (oldest first) on top of `requests`, newest first, up to 100 */
export function addAll(requests: Served[], arrived: Served[]): Served[] {
  return [...arrived.slice(-SHOWN).reverse(), ...requests].slice(0, SHOWN);
}

/** How often the list changes: fast enough to feel live, slow enough to read */
const REFRESH_MS = 1000;

/**
 * The latest requests. `paused` stops watching; `held` (the pointer is on the list) keeps the
 * list still, and what arrives meanwhile is shown when it's let go.
 */
export function useRequests(paused: boolean, held: boolean, host?: string): { requests: Served[]; skipped: number } {
  const [requests, setRequests] = useState<Served[]>([]);
  const [skipped, setSkipped] = useState(0);
  const arrived = useRef<Served[]>([]);
  const holding = useRef(held);
  holding.current = held;

  useEffect(() => {
    if (paused) return;
    const source = new EventSource(`/api/v1/requests/stream${host ? `?host=${encodeURIComponent(host)}` : ''}`);
    source.onmessage = (event) => {
      arrived.current.push(JSON.parse(event.data) as Served);
      if (arrived.current.length > SHOWN) arrived.current.splice(0, arrived.current.length - SHOWN);
    };
    source.addEventListener('skipped', (event) => setSkipped(Number((event as MessageEvent).data)));
    const timer = setInterval(() => {
      if (holding.current || arrived.current.length === 0) return;
      const batch = arrived.current;
      arrived.current = [];
      setRequests((current) => addAll(current, batch));
    }, REFRESH_MS);
    return () => {
      source.close();
      clearInterval(timer);
    };
  }, [paused, host]);
  return { requests, skipped };
}
