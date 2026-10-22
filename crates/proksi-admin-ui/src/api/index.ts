import type { Rule } from '../features/rules/ruleTree';

// The admin API. Signing in exchanges a token for a session cookie (HttpOnly): the page never
// keeps the token.

export class Unauthorized extends Error {}

/** An answer other than a success, with its status */
export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

/** The host is on another version than the change was made from (`412`): reload it */
export class Conflict extends Error {
  constructor(
    message: string,
    /** The host's active version now (0: it isn't in the store) */
    readonly version: number,
  ) {
    super(message);
  }
}

/** What a change was made from: the host's active version as read, or no host */
export type Expect = { version: number } | 'absent';

function conditions(expect?: Expect): Record<string, string> {
  if (expect === undefined) return {};
  if (expect === 'absent') return { 'if-none-match': '*' };
  return { 'if-match': `"${expect.version}"` };
}

async function request<T>(method: string, path: string, body?: unknown, extra: Record<string, string> = {}): Promise<T> {
  const headers: Record<string, string> = { accept: 'application/json', ...extra };
  if (body !== undefined) headers['content-type'] = 'application/json';
  const response = await fetch(`/api/v1${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: 'same-origin',
  });
  if (response.status === 401) throw new Unauthorized();
  const json = await response.json().catch(() => ({}));
  if (response.status === 412) throw new Conflict(json.error ?? 'changed by someone else', json.version ?? 0);
  if (!response.ok) throw new ApiError(json.error ?? `${response.status} ${response.statusText}`, response.status);
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

/** A change to a host, newest first in its history */
export type HistoryEntry = {
  revision: number;
  /** Unix timestamp (seconds) */
  at: number;
  author: string | null;
} & ({ change: 'saved' } | { change: 'deleted' } | { change: 'rolled_back'; to: number });

/** A saved definition of a host */
export type SavedVersion = { host: StoredHost; version: number; at: number; author: string | null };

type Written = { host: string; revision: number };

export const routes = {
  /** The host, with its active version (the revision its definition was saved at) */
  get: (name: string) => get<{ host: StoredHost; revision: number; version: number }>(hostPath(name)),
  put: (host: StoredHost, expect?: Expect) => request<Written>('PUT', hostPath(host.name), host, conditions(expect)),
  delete: (name: string, expect?: Expect) => request<Written>('DELETE', hostPath(name), undefined, conditions(expect)),
  /** Checks a host as saving would, without saving it */
  validate: (host: StoredHost) => request<{ valid: boolean }>('POST', '/validate', host),
  /** Its changes, newest first: none for a host saved before versions were kept */
  history: (name: string) =>
    get<{ host: string; changes: HistoryEntry[] }>(`${hostPath(name)}/history`).catch((err) => {
      if (err instanceof ApiError && err.status === 404) return { host: name, changes: [] };
      throw err;
    }),
  version: (name: string, revision: number) => get<SavedVersion>(`${hostPath(name)}/history/${revision}`),
  /** Makes a version active again (also restores a deleted host) */
  rollback: (name: string, revision: number, expect?: Expect) =>
    request<Written>('POST', `${hostPath(name)}/rollback`, { revision }, conditions(expect)),
  /** Deleted hosts that still have versions */
  deleted: () => get<{ hosts: string[] }>('/deleted-routes'),
};

export const session = {
  get: () => get<Session>('/session'),
  signIn: (name: string, token: string) => request<Session>('POST', '/session', { name, token }),
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
/** How the latest send to a webhook went */
export type Delivery = { event: string; /** Unix timestamp (seconds) */ at: number; ok: boolean; error?: string };
/** A `notify` block, without its secrets: the URL masked, headers by name */
export type Notification = {
  name: string;
  source: 'file' | 'store';
  url: string;
  format: 'json' | 'slack' | 'discord';
  /** The events it's sent (none: all of them) */
  on: string[];
  /** The hosts whose events it's sent (none: every host) */
  hosts: string[];
  headers: string[];
  expiring_days: number;
  last: Delivery | null;
};
export type Notifications = { notifications: Notification[]; events: string[] };

export const notifications = {
  /** Sends a `test` event to the block's webhook: rejected with the error when it fails */
  test: (name: string) => request<{ ok: true }>('POST', `/notifications/${encodeURIComponent(name)}/test`),
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
