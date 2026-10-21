import { describe, expect, it } from 'vitest';
import type { StoredHost } from '../../api';
import { emptyHost, fromForm, parseAddress, toForm } from './hostForm';

const stored: StoredHost = {
  name: 'shop.example.com',
  listen: [],
  tls: { enabled: true, self_signed_on_failure: true, certificate: null },
  rules: [{ name: 'writes', and: { method: { eq: 'POST' } }, then: { respond: { status: 403 } } }],
  locations: [
    {
      path: '/api',
      exact: false,
      plugins: [{ name: 'basic_auth' }],
      upstreams: [
        { ip: 'app.internal', port: 8080, weight: 2, sni: null, network: null },
        { ip: '[::1]', port: 9000, weight: null, sni: 'app.test' },
      ],
    },
  ],
};

describe('the host form', () => {
  it('saves a host as it was read', () => {
    expect(fromForm(toForm(stored))).toEqual({ host: stored });
  });

  it('keeps what it does not edit', () => {
    const form = toForm(stored);
    form.locations[0].upstreams[0].address = 'other.internal:80';
    const saved = fromForm(form);
    if (!('host' in saved)) throw new Error(String(saved.errors));
    expect(saved.host.rules).toEqual(stored.rules);
    expect(saved.host.locations[0].plugins).toEqual(stored.locations[0].plugins);
    expect(saved.host.locations[0].upstreams[0]).toMatchObject({ ip: 'other.internal', port: 80, network: null });
  });

  it('keeps rules, and leaves out empty ones', () => {
    const form = toForm(stored);
    expect(form.rules).toEqual(stored.rules);
    form.locations[0].rules = [{ name: 'a' }];
    form.rules = [];
    const saved = fromForm(form);
    if (!('host' in saved)) throw new Error(String(saved.errors));
    expect(saved.host.rules).toBeUndefined();
    expect(saved.host.locations[0].rules).toEqual([{ name: 'a' }]);
  });

  it('refuses what the API would', () => {
    const form = emptyHost();
    form.locations[0].path = 'api';
    form.locations[0].upstreams[0].address = 'app';
    expect(fromForm(form)).toEqual({
      errors: [
        'The host needs a name.',
        'Location api: the path must start with /.',
        'Location api: "app" isn\'t an address (host:port).',
      ],
    });
  });

  it('reads addresses', () => {
    expect(parseAddress('app:80')).toEqual({ ip: 'app', port: 80 });
    expect(parseAddress('[::1]:443')).toEqual({ ip: '[::1]', port: 443 });
    for (const invalid of ['app', 'app:0', 'app:70000', ':80', 'a b:80']) expect(parseAddress(invalid)).toBeNull();
  });
});
