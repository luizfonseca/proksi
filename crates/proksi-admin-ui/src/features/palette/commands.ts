import type { Host } from '../../api';
import { DOCS, GITHUB } from '../../lib/links';

/** The groups of the palette, in the order they're shown */
export const GROUPS = ['Hosts', 'Pages', 'Actions'] as const;

/** Icons of pages and actions (hosts show their health) */
export type IconName = 'overview' | 'hosts' | 'certificates' | 'notifications' | 'server' | 'new' | 'edit' | 'history' | 'docs' | 'github' | 'sign-out';

/** Something the palette opens or does */
export type Command = {
  /** What it's called, and what it's found by */
  title: string;
  group: (typeof GROUPS)[number];
  /** Shown at its right, in monospace */
  detail?: string;
  icon?: IconName;
  /** A host's health: its upstreams all up, some down, or none */
  health?: 'up' | 'down' | 'none';
  /** Other words it's found by */
  keywords?: string;
  /** Shown only once something is typed (each host's editor and history) */
  searched?: boolean;
  /** A page of the UI, or something to do */
  run: { href: string } | { url: string } | { action: 'sign-out' };
};

export const PAGES: Command[] = [
  { title: 'Overview', group: 'Pages', icon: 'overview', keywords: 'dashboard metrics live', run: { href: '/overview' } },
  { title: 'Hosts', group: 'Pages', icon: 'hosts', keywords: 'routes sites', run: { href: '/hosts' } },
  { title: 'Certificates', group: 'Pages', icon: 'certificates', keywords: 'tls https letsencrypt', run: { href: '/certificates' } },
  { title: 'Notifications', group: 'Pages', icon: 'notifications', keywords: 'notify alerts slack discord', run: { href: '/notifications' } },
  { title: 'Server', group: 'Pages', icon: 'server', keywords: 'settings configuration', run: { href: '/server' } },
];

export const ACTIONS: Command[] = [
  { title: 'Documentation', group: 'Actions', icon: 'docs', keywords: 'docs help guide support', run: { url: DOCS } },
  { title: 'Proksi on GitHub', group: 'Actions', icon: 'github', keywords: 'source code issues support', run: { url: GITHUB } },
  { title: 'Sign out', group: 'Actions', icon: 'sign-out', keywords: 'log out', run: { action: 'sign-out' } },
];

function health(host: Host): Command['health'] {
  const upstreams = host.locations.flatMap((location) => location.upstreams);
  if (upstreams.length === 0) return 'none';
  return upstreams.every((upstream) => upstream.healthy) ? 'up' : 'down';
}

/** Each host's page, and its editor and history when it's in the store and `canWrite` */
export function hostCommands(hosts: Host[], canWrite: boolean): Command[] {
  const commands: Command[] = [];
  const actions: Command[] = canWrite ? [{ title: 'New host', group: 'Actions', icon: 'new', keywords: 'add create', run: { href: '/hosts/new' } }] : [];
  for (const host of hosts) {
    const page = `/hosts/${encodeURIComponent(host.host)}`;
    commands.push({ title: host.host, group: 'Hosts', detail: host.source ?? undefined, health: health(host), run: { href: page } });
    if (canWrite && host.source === 'store') {
      actions.push({ title: `Edit ${host.host}`, group: 'Actions', icon: 'edit', searched: true, run: { href: `${page}/edit` } });
      actions.push({ title: `History of ${host.host}`, group: 'Actions', icon: 'history', keywords: 'versions rollback', searched: true, run: { href: `${page}/history` } });
    }
  }
  return [...commands, ...actions];
}

/**
 * How well `query` matches `command`: 0 when it doesn't. Each word of the query must start its
 * title (best), start a word of its title or keywords, or be found in them anywhere.
 */
export function score(command: Command, query: string): number {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return command.searched ? 0 : 1;
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

/** The commands matching `query`, by group, best first in each (in their order when as good) */
export function search(commands: Command[], query: string, perGroup = 6): Command[] {
  const scored = commands
    .map((command, index) => ({ command, index, score: score(command, query) }))
    .filter((found) => found.score > 0)
    .sort((a, b) => b.score - a.score || a.index - b.index);
  return GROUPS.flatMap((group) =>
    scored
      .filter((found) => found.command.group === group)
      .slice(0, perGroup)
      .map((found) => found.command),
  );
}
