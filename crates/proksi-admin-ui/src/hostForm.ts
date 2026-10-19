import type { StoredHost, StoredLocation, StoredUpstream } from './api';

// The host editor's form: the settings it edits, and everything else kept as it was

export type UpstreamForm = { address: string; weight: string; sni: string; rest: Record<string, unknown> };
export type LocationForm = {
  path: string;
  exact: boolean;
  upstreams: UpstreamForm[];
  /** As Proksi takes them, in order */
  rules: unknown[];
  rest: Record<string, unknown>;
};
export type HostForm = {
  name: string;
  tls: boolean;
  selfSigned: boolean;
  rules: unknown[];
  locations: LocationForm[];
  tlsRest: Record<string, unknown>;
  rest: Record<string, unknown>;
};

export const emptyUpstream = (): UpstreamForm => ({ address: '', weight: '', sni: '', rest: {} });
export const emptyLocation = (): LocationForm => ({ path: '/', exact: false, upstreams: [emptyUpstream()], rules: [], rest: {} });

export function emptyHost(): HostForm {
  return { name: '', tls: true, selfSigned: false, rules: [], locations: [emptyLocation()], tlsRest: {}, rest: {} };
}

const list = (rules: unknown) => (Array.isArray(rules) ? rules : []);

export function toForm(host: StoredHost): HostForm {
  const { name, tls, locations, rules, ...rest } = host;
  const { enabled, self_signed_on_failure, ...tlsRest } = tls ?? { enabled: true };
  return {
    name,
    tls: enabled !== false,
    selfSigned: Boolean(self_signed_on_failure),
    rules: list(rules),
    tlsRest,
    rest,
    locations: (locations ?? []).map(({ path, exact, upstreams, rules, ...rest }) => ({
      path,
      exact: Boolean(exact),
      rules: list(rules),
      rest,
      upstreams: (upstreams ?? []).map(({ ip, port, weight, sni, ...rest }) => ({
        address: `${ip}:${port}`,
        weight: weight == null ? '' : String(weight),
        sni: sni ?? '',
        rest,
      })),
    })),
  };
}

/** `host:port`, `ip:port` or `[ipv6]:port` */
export function parseAddress(address: string): { ip: string; port: number } | null {
  const match = /^(\[[^\]]+\]|[^:\s]+):(\d{1,5})$/.exec(address.trim());
  if (!match) return null;
  const port = Number(match[2]);
  if (port < 1 || port > 65535) return null;
  return { ip: match[1], port };
}

/** The host to store, or the errors that keep it from being saved */
export function fromForm(form: HostForm): { host: StoredHost } | { errors: string[] } {
  const errors: string[] = [];
  const name = form.name.trim().toLowerCase();
  if (!name) errors.push('The host needs a name.');
  const locations: StoredLocation[] = form.locations.map((location, index) => {
    const at = `Location ${location.path || index + 1}`;
    if (!location.path.startsWith('/')) errors.push(`${at}: the path must start with /.`);
    const upstreams: StoredUpstream[] = location.upstreams
      .filter((upstream) => upstream.address.trim())
      .map((upstream) => {
        const address = parseAddress(upstream.address);
        if (!address) errors.push(`${at}: "${upstream.address}" isn't an address (host:port).`);
        const weight = upstream.weight.trim() === '' ? null : Number(upstream.weight);
        if (weight !== null && !(Number.isInteger(weight) && weight >= 1 && weight <= 127)) {
          errors.push(`${at}: the weight of ${upstream.address} must be from 1 to 127.`);
        }
        return {
          ...upstream.rest,
          ip: address?.ip ?? '',
          port: address?.port ?? 0,
          weight,
          sni: upstream.sni.trim() || null,
        };
      });
    const rules = location.rules.length ? { rules: location.rules } : {};
    return { ...location.rest, path: location.path, exact: location.exact, upstreams, ...rules };
  });
  const rules = form.rules.length ? { rules: form.rules } : {};
  if (errors.length) return { errors };
  return {
    host: {
      ...form.rest,
      name,
      tls: { ...form.tlsRest, enabled: form.tls, self_signed_on_failure: form.selfSigned },
      ...rules,
      locations,
    },
  };
}
