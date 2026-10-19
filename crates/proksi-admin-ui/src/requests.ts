// The latest requests, from `/api/v1/requests/stream`: watched only while the overview is open,
// since Proksi builds them only while someone watches

import { useEffect, useState } from 'preact/hooks';

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

export function useRequests(paused: boolean): { requests: Served[]; skipped: number } {
  const [requests, setRequests] = useState<Served[]>([]);
  const [skipped, setSkipped] = useState(0);
  useEffect(() => {
    if (paused) return;
    const source = new EventSource('/api/v1/requests/stream');
    source.onmessage = (event) => setRequests((current) => add(current, JSON.parse(event.data) as Served));
    source.addEventListener('skipped', (event) => setSkipped(Number((event as MessageEvent).data)));
    return () => source.close();
  }, [paused]);
  return { requests, skipped };
}
