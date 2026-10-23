import type { Notification } from '../../api';

// A `notify` block as the form edits it. The API never sends back a block's URL or its headers'
// values: left empty, they keep what's saved.

/** `saved`: the block has it, so an empty value keeps the saved one */
export type HeaderForm = { name: string; value: string; saved?: boolean };

export type NotifyForm = {
  name: string;
  url: string;
  format: Notification['format'];
  /** None: every event */
  on: string[];
  /** None: every host */
  hosts: string[];
  headers: HeaderForm[];
  expiring_days: string;
};

/** What's sent to save a block */
export type NotifyBody = {
  url: string;
  format: Notification['format'];
  on: string[];
  hosts: string[];
  headers: Record<string, string>;
  expiring_days: number;
};

export function emptyNotify(): NotifyForm {
  return { name: '', url: '', format: 'json', on: [], hosts: [], headers: [], expiring_days: '14' };
}

/** A saved block, to change */
export function toForm(block: Notification): NotifyForm {
  return {
    name: block.name,
    url: '',
    format: block.format,
    on: [...block.on],
    hosts: [...block.hosts],
    headers: block.headers.map((name) => ({ name, value: '', saved: true })),
    expiring_days: String(block.expiring_days),
  };
}

/** The block to save, or what to fix. `saved`: changing a saved block (an empty URL keeps its own) */
export function fromForm(form: NotifyForm, saved: boolean): { name: string; body: NotifyBody } | { errors: string[] } {
  const errors: string[] = [];
  const name = form.name.trim();
  const url = form.url.trim();
  if (!/^[A-Za-z0-9._-]{1,64}$/.test(name)) errors.push('The name can only have letters, digits, -, _ and .');
  if (!url && !saved) errors.push('The URL is required');
  if (url && !/^https?:\/\//.test(url)) errors.push('The URL must start with https:// or http://');
  const headers: Record<string, string> = {};
  for (const header of form.headers.filter((header) => header.name.trim())) {
    if (!header.value && !header.saved) errors.push(`The header ${header.name.trim()} needs a value`);
    headers[header.name.trim().toLowerCase()] = header.value;
  }
  const days = Number(form.expiring_days);
  if (!Number.isInteger(days) || days < 1 || days > 60) errors.push('Days before expiry must be between 1 and 60');
  if (errors.length) return { errors };
  return { name, body: { url, format: form.format, on: form.on, hosts: form.hosts, headers, expiring_days: days } };
}
