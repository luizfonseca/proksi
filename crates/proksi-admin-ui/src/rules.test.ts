import { describe, expect, it } from 'vitest';
import { actions, condition } from './rules';

describe('rules', () => {
  it('reads conditions as a tree', () => {
    expect(
      condition({
        name: 'writes',
        and: {
          method: { not_in: ['GET', 'HEAD'] },
          header: { 'x-partner': { exists: true } },
          or: { path: { prefix: '/admin' }, not: { client: { in: ['10.0.0.0/8'] } } },
        },
        then: { respond: { status: 403 } },
      }),
    ).toEqual([
      {
        group: 'all',
        children: [
          { test: 'method is none of GET, HEAD' },
          { test: 'header "x-partner" is there' },
          {
            group: 'any',
            children: [
              { test: 'path starts with /admin' },
              { group: 'none', children: [{ test: 'client is one of 10.0.0.0/8' }] },
            ],
          },
        ],
      },
    ]);
  });

  it('reads repeated blocks and tests as several', () => {
    expect(condition({ or: [{ path: { eq: '/a' } }, { path: { eq: '/b' } }] })).toEqual([
      { group: 'any', children: [{ test: 'path is /a' }] },
      { group: 'any', children: [{ test: 'path is /b' }] },
    ]);
    expect(condition({ and: { path: [{ prefix: '/a' }, { suffix: '.json' }] } })).toEqual([
      { group: 'all', children: [{ test: 'path starts with /a' }, { test: 'path ends with .json' }] },
    ]);
  });

  it('reads actions with their settings', () => {
    expect(
      actions({ then: { rate_limit: { requests: 30, per: '1m' }, basic_auth: '(hidden)' } }),
    ).toEqual([
      { block: 'rate_limit', settings: 'requests 30, per 1m' },
      { block: 'basic_auth', settings: 'settings hidden' },
    ]);
  });
});
