import { describe, expect, it } from 'vitest';
import type { Host } from '../../api';
import { ACTIONS, hostCommands, PAGES, search } from './commands';

const host = (name: string, source: string | null): Host => ({ host: name, source, tls: true, listen: [], rules: [], locations: [] });
const titles = (found: { title: string }[]) => found.map((command) => command.title);

describe('the palette', () => {
  const hosts = [host('api.example.com', 'store'), host('shop.example.com', 'file'), host('db.internal', 'docker')];

  it('finds pages and hosts by the start of their words first', () => {
    const commands = [...PAGES, ...hostCommands(hosts, true), ...ACTIONS];
    // A title's start, then a word's start, then anywhere
    expect(titles(search(commands, 'ho'))).toEqual(['Hosts', 'New host', 'shop.example.com']);
    expect(titles(search(commands, 'hist'))).toEqual(['api.example.com: history']);
    expect(titles(search(commands, 'shop'))).toEqual(['shop.example.com']);
    expect(titles(search(commands, 'api edit'))).toEqual(['api.example.com: edit']);
    // By keyword, and anywhere in a name
    expect(titles(search(commands, 'tls'))).toEqual(['Certificates']);
    expect(titles(search(commands, 'internal'))).toEqual(['db.internal']);
    expect(search(commands, 'nothing like it')).toEqual([]);
  });

  it('shows everything for an empty query, in order', () => {
    expect(titles(search(PAGES, '  '))).toEqual(['Overview', 'Hosts', 'Certificates', 'Server']);
  });

  it('offers editing only for hosts in the store, and only to who can write', () => {
    expect(titles(hostCommands(hosts, true))).toEqual([
      'New host',
      'api.example.com',
      'api.example.com: edit',
      'api.example.com: history',
      'shop.example.com',
      'db.internal',
    ]);
    expect(titles(hostCommands(hosts, false))).toEqual(['api.example.com', 'shop.example.com', 'db.internal']);
  });

  it('takes queries as text, not patterns', () => {
    expect(titles(search(hostCommands([host('a+b.test', null)], false), 'a+b'))).toEqual(['a+b.test']);
  });
});
