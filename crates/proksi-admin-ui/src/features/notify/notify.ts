import type { Notification } from '../../api';

/** `*` matches every host, `*.example.com` one more label (as wildcard hosts do), anything else
 * the host with that name: as Proksi matches a `notify` block's `hosts` */
export function hostMatches(pattern: string, host: string): boolean {
  if (pattern === '*') return true;
  const name = host.toLowerCase();
  if (pattern.startsWith('*.')) {
    const dot = name.indexOf('.');
    return dot > 0 && name.slice(dot + 1) === pattern.slice(2).toLowerCase();
  }
  return pattern.toLowerCase() === name;
}

/** The blocks told about `host`'s events */
export function notifying(blocks: Notification[], host: string): Notification[] {
  return blocks.filter((block) => block.hosts.length === 0 || block.hosts.some((pattern) => hostMatches(pattern, host)));
}

/** The hosts among `hosts` whose events `block` is told about */
export function covered(block: Notification, hosts: string[]): string[] {
  return hosts.filter((host) => notifying([block], host).length > 0);
}
