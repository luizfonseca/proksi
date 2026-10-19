// The admin API. Until sessions exist (ADR 0008 step 2), a token is kept in memory only: it's
// gone when the page is closed or reloaded.

let token: string | null = null;

export function setToken(value: string | null) {
  token = value;
}

export class Unauthorized extends Error {}

export async function get<T>(path: string): Promise<T> {
  const headers: Record<string, string> = { accept: 'application/json' };
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fetch(`/api/v1${path}`, { headers });
  if (response.status === 401) throw new Unauthorized();
  const body = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(body.error ?? `${response.status} ${response.statusText}`);
  return body as T;
}

export type Upstream = { upstream: string; weight: number; healthy: boolean };
export type Location = { location: string; blocks: string[]; upstreams: Upstream[] };
export type Host = {
  host: string;
  source: string | null;
  tls: boolean;
  listen: string[];
  locations: Location[];
};
export type Certificate = {
  domain: string;
  issuer: string | null;
  self_signed: boolean;
  expires_at: string | null;
  days_left: number | null;
  lease_holder: string | null;
};
export type Status = {
  version: string;
  hostname: string;
  uptime_seconds: number;
  store: { type: string; revision: number | null; applied_revision: number | null; error: string | null };
  routes: { by_source: Record<string, number>; conflicts: unknown[]; invalid: unknown[] };
};
