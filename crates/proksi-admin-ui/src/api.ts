// The admin API. Signing in exchanges a token for a session cookie (HttpOnly): the page never
// keeps the token.

export class Unauthorized extends Error {}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { accept: 'application/json' };
  if (body !== undefined) headers['content-type'] = 'application/json';
  const response = await fetch(`/api/v1${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: 'same-origin',
  });
  if (response.status === 401) throw new Unauthorized();
  const json = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(json.error ?? `${response.status} ${response.statusText}`);
  return json as T;
}

export function get<T>(path: string): Promise<T> {
  return request<T>('GET', path);
}

/** Who's signed in: a token's name (none when Proksi has no tokens) and what it can do */
export type Session = { name: string | null; access: 'read' | 'write' };

/** A host in the store, as the API stores it */
export type StoredUpstream = {
  ip: string;
  port: number;
  weight?: number | null;
  sni?: string | null;
  [key: string]: unknown;
};
export type StoredLocation = { path: string; exact?: boolean; upstreams: StoredUpstream[]; [key: string]: unknown };
export type StoredHost = {
  name: string;
  listen?: string[];
  tls: { enabled: boolean; self_signed_on_failure?: boolean; [key: string]: unknown };
  locations: StoredLocation[];
  [key: string]: unknown;
};

const hostPath = (name: string) => `/routes/${encodeURIComponent(name)}`;

export const routes = {
  get: (name: string) => get<{ host: StoredHost; revision: number }>(hostPath(name)),
  put: (host: StoredHost) => request<{ host: string; revision: number }>('PUT', hostPath(host.name), host),
  delete: (name: string) => request<{ host: string; revision: number }>('DELETE', hostPath(name)),
};

export const session = {
  get: () => get<Session>('/session'),
  signIn: (token: string) => request<Session>('POST', '/session', { token }),
  signOut: () => request<unknown>('DELETE', '/session'),
};

export type Upstream = {
  /** As written (`ip:port` or `name:port`) */
  upstream: string;
  /** What it came from: a docker label's upstream, or a Kubernetes service */
  name: string | null;
  weight: number;
  healthy: boolean;
  /** What it resolved to */
  addresses: { address: string; healthy: boolean }[];
};
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
