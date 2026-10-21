import type { Rule } from '../features/rules/ruleTree';

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
  /** Checks a host as saving would, without saving it */
  validate: (host: StoredHost) => request<{ valid: boolean }>('POST', '/validate', host),
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
  /** The traffic split it's in, if any */
  split?: { name: string; percent: number };
  /** An upstream of the location's mirror */
  mirror?: boolean;
};
/** Where copies of a location's requests go, and what became of them */
export type Mirror = { percent: number; sent: number; failed: number; dropped: number; too_large: number };
export type Location = { location: string; blocks: string[]; rules: Rule[]; upstreams: Upstream[]; mirror?: Mirror };
export type Host = {
  host: string;
  source: string | null;
  tls: boolean;
  listen: string[];
  /** Without secrets: hidden values read `(hidden)` */
  rules: Rule[];
  locations: Location[];
};
/** A `tcp` block: connections accepted on `listen`, forwarded to its upstreams */
export type TcpBlock = {
  /** As the stats name it (`tcp "db" in host "a.com"`) */
  block: string;
  name: string;
  host: string | null;
  listen: string;
  tls: 'none' | 'terminate' | 'passthrough';
  alpn: string[];
  upstreams: Upstream[];
};
export type Certificate = {
  domain: string;
  issuer: string | null;
  self_signed: boolean;
  expires_at: string | null;
  days_left: number | null;
  lease_holder: string | null;
};
export type Replica = { id: string; revision: number; applied_at: number; version: string; hostname: string };
export type Status = {
  version: string;
  replica: string;
  hostname: string;
  uptime_seconds: number;
  store: {
    type: string;
    revision: number | null;
    applied_revision: number | null;
    replicas: Replica[];
    error: string | null;
  };
  routes: { by_source: Record<string, number>; conflicts: unknown[]; invalid: unknown[] };
};
