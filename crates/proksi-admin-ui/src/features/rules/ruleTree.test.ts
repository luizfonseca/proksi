import { describe, expect, it } from 'vitest';
import { actions, condition } from './ruleTree';

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
          { test: { field: 'method', operator: 'is none of', value: 'GET, HEAD' } },
          { test: { field: 'header', name: 'x-partner', operator: 'is there' } },
          {
            group: 'any',
            children: [
              { test: { field: 'path', operator: 'starts with', value: '/admin' } },
              { group: 'none', children: [{ test: { field: 'client', operator: 'is one of', value: '10.0.0.0/8' } }] },
            ],
          },
        ],
      },
    ]);
  });

  it('reads repeated blocks and tests as several', () => {
    expect(condition({ or: [{ path: { eq: '/a' } }, { path: { eq: '/b' } }] })).toEqual([
      { group: 'any', children: [{ test: { field: 'path', operator: 'is', value: '/a' } }] },
      { group: 'any', children: [{ test: { field: 'path', operator: 'is', value: '/b' } }] },
    ]);
    expect(condition({ and: { path: [{ prefix: '/a' }, { suffix: '.json' }] } })).toEqual([
      {
        group: 'all',
        children: [
          { test: { field: 'path', operator: 'starts with', value: '/a' } },
          { test: { field: 'path', operator: 'ends with', value: '.json' } },
        ],
      },
    ]);
  });

  it('keeps hidden values hidden', () => {
    expect(condition({ and: { header: { 'x-staff-token': { neq: '(hidden)' } } } })).toEqual([
      { group: 'all', children: [{ test: { field: 'header', name: 'x-staff-token', operator: 'is not', hidden: true } }] },
    ]);
  });

  it('reads actions with their settings', () => {
    expect(
      actions({ then: { rate_limit: { requests: 30, per: '1m' }, basic_auth: '(hidden)', request_id: {} } }),
    ).toEqual([
      { block: 'rate_limit', settings: [['requests', '30'], ['per', '1m']] },
      { block: 'basic_auth', settings: 'hidden' },
      { block: 'request_id', settings: [] },
    ]);
    expect(actions({ then: { headers: { request: { add: { 'x-partner': '(hidden)' } } } } })).toEqual([
      { block: 'headers', settings: [['request', 'add x-partner (hidden)']] },
    ]);
  });
});
