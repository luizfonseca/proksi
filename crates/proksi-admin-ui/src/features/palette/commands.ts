import type { Host } from '../../api';

/** Something the palette opens or does */
export type Command = {
  /** What it's called, and what it's found by */
  title: string;
  /** What kind of thing it is, shown beside it */
  group: 'Page' | 'Host' | 'Action';
  /** Other words it's found by */
  keywords?: string;
  /** A page of the UI, or something to do */
  run: { href: string } | { action: 'theme' | 'sign-out' };
};

export const PAGES: Command[] = [
  { title: 'Overview', group: 'Page', keywords: 'dashboard metrics live', run: { href: '/overview' } },
  { title: 'Hosts', group: 'Page', keywords: 'routes sites', run: { href: '/hosts' } },
  { title: 'Certificates', group: 'Page', keywords: 'tls https letsencrypt', run: { href: '/certificates' } },
  { title: 'Server', group: 'Page', keywords: 'settings configuration', run: { href: '/server' } },
];

export const ACTIONS: Command[] = [
  { title: 'Switch theme', group: 'Action', keywords: 'dark light mode', run: { action: 'theme' } },
  { title: 'Sign out', group: 'Action', keywords: 'log out', run: { action: 'sign-out' } },
];

/** Each host's page, and its history and editor when it's in the store and `canWrite` */
export function hostCommands(hosts: Host[], canWrite: boolean): Command[] {
  const commands: Command[] = canWrite ? [{ title: 'New host', group: 'Action', keywords: 'add create', run: { href: '/hosts/new' } }] : [];
  for (const host of hosts) {
    const page = `/hosts/${encodeURIComponent(host.host)}`;
    commands.push({ title: host.host, group: 'Host', keywords: host.source ?? '', run: { href: page } });
    if (canWrite && host.source === 'store') {
      commands.push({ title: `${host.host}: edit`, group: 'Host', run: { href: `${page}/edit` } });
      commands.push({ title: `${host.host}: history`, group: 'Host', keywords: 'versions rollback', run: { href: `${page}/history` } });
    }
  }
  return commands;
}

/**
 * How well `query` matches `command`: 0 when it doesn't. Each word of the query must start a word
 * of its title or keywords (best), or be found in it anywhere.
 */
export function score(command: Command, query: string): number {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return 1;
  const title = command.title.toLowerCase();
  const text = `${title} ${command.keywords?.toLowerCase() ?? ''}`;
  let total = 0;
  for (const word of words) {
    if (title.startsWith(word)) total += 4;
    else if (new RegExp(`(^|[^a-z0-9])${escape(word)}`).test(text)) total += 3;
    else if (text.includes(word)) total += 1;
    else return 0;
  }
  return total;
}

function escape(word: string) {
  return word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** The commands matching `query`, best first (in their order when as good) */
export function search(commands: Command[], query: string, limit = 12): Command[] {
  return commands
    .map((command, index) => ({ command, index, score: score(command, query) }))
    .filter((found) => found.score > 0)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .slice(0, limit)
    .map((found) => found.command);
}
