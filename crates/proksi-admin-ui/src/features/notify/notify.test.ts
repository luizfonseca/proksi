import { describe, expect, it } from 'vitest';
import type { Notification } from '../../api';
import { covered, hostMatches, notifying } from './notify';

const block = (name: string, hosts: string[]): Notification => ({
  name,
  source: 'file',
  version: null,
  url: 'https://hook…bf3a',
  format: 'json',
  on: [],
  hosts,
  headers: [],
  expiring_days: 14,
  last: null,
});

describe('notify blocks', () => {
  it('match hosts as Proksi does', () => {
    expect(hostMatches('*', 'a.example.com')).toBe(true);
    expect(hostMatches('api.example.com', 'API.example.com')).toBe(true);
    expect(hostMatches('*.example.com', 'a.example.com')).toBe(true);
    expect(hostMatches('*.example.com', 'example.com')).toBe(false);
    expect(hostMatches('*.example.com', 'a.b.example.com')).toBe(false);
  });

  it('are told about a host when they list it, or no hosts at all', () => {
    const blocks = [block('ops', []), block('payments', ['pay.example.com']), block('shop', ['*.shop.example.com'])];
    expect(notifying(blocks, 'pay.example.com').map((found) => found.name)).toEqual(['ops', 'payments']);
    expect(notifying(blocks, 'eu.shop.example.com').map((found) => found.name)).toEqual(['ops', 'shop']);
  });

  it('cover the hosts their patterns match, or every host without any', () => {
    const hosts = ['pay.example.com', 'db.internal', 'web.internal', 'internal'];
    expect(covered(block('payments', ['pay.example.com', '*.internal']), hosts)).toEqual(['pay.example.com', 'db.internal', 'web.internal']);
    expect(covered(block('ops', []), hosts)).toEqual(hosts);
    expect(covered(block('none', ['shop.example.com']), hosts)).toEqual([]);
  });
});
