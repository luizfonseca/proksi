import { describe, expect, it } from 'vitest';
import type { Host } from '../../api';
import { ACTIONS, hostCommands, PAGES, search } from './commands';

const host = (name: string, source: string | null, healthy: boolean[] = []): Host => ({
  host: name,
  source,
  tls: true,
  listen: [],
  rules: [],
  locations: [
    {
      location: '/',
      blocks: [],
      rules: [],
      upstreams: healthy.map((up, at) => ({ upstream: `10.0.0.${at}:80`, healthy: up }) as Host['locations'][number]['upstreams'][number]),
    },
  ],
});
const titles = (found: { title: string }[]) => found.map((command) => command.title);

describe('the palette', () => {
  const hosts = [host('api.example.com', 'store', [true]), host('shop.example.com', 'file', [true, false]), host('db.internal', 'docker')];
  const commands = [...PAGES, ...hostCommands(hosts, true), ...ACTIONS];

  it('shows hosts, then pages, then actions, the best first in each', () => {
    // A title's start, then a word's start, then anywhere
    expect(titles(search(commands, 'ho'))).toEqual(['shop.example.com', 'Hosts', 'New host']);
    expect(titles(search(commands, 'shop'))).toEqual(['shop.example.com']);
    expect(titles(search(commands, 'edit api'))).toEqual(['Edit api.example.com']);
    // By keyword, and anywhere in a name
    expect(titles(search(commands, 'tls'))).toEqual(['Certificates']);
    expect(titles(search(commands, 'internal'))).toEqual(['db.internal']);
    expect(search(commands, 'nothing like it')).toEqual([]);
  });

  it("shows everything but each host's editor and history before anything is typed", () => {
    expect(titles(search(commands, '  '))).toEqual([
      'api.example.com',
      'shop.example.com',
      'db.internal',
      'Overview',
      'Hosts',
      'Certificates',
      'Server',
      'New host',
      'Sign out',
    ]);
  });

  it("shows each host's health and source", () => {
    expect(hostCommands(hosts, false).map(({ health, detail }) => [health, detail])).toEqual([
      ['up', 'store'],
      ['down', 'file'],
      ['none', 'docker'],
    ]);
  });

  it('offers editing only for hosts in the store, and only to who can write', () => {
    expect(titles(hostCommands(hosts, true))).toEqual([
      'api.example.com',
      'shop.example.com',
      'db.internal',
      'New host',
      'Edit api.example.com',
      'History of api.example.com',
    ]);
    expect(titles(hostCommands(hosts, false))).toEqual(['api.example.com', 'shop.example.com', 'db.internal']);
  });

  it('takes queries as text, not patterns', () => {
    expect(titles(search(hostCommands([host('a+b.test', null)], false), 'a+b'))).toEqual(['a+b.test']);
  });
});
