import { describe, expect, it } from 'vitest';
import type { Notification } from '../../api';
import { emptyNotify, fromForm, toForm } from './notifyForm';

const saved: Notification = {
  name: 'ops',
  source: 'store',
  version: 4,
  url: 'https://hook…bf3a',
  format: 'slack',
  on: ['upstream_down'],
  hosts: ['*.example.com'],
  headers: ['authorization'],
  expiring_days: 14,
  last: null,
};

describe('the notify form', () => {
  it('starts a new block with every event and host', () => {
    const form = { ...emptyNotify(), name: 'ops', url: 'https://hooks.example.com/a' };
    expect(fromForm(form, false)).toEqual({
      name: 'ops',
      body: { url: 'https://hooks.example.com/a', format: 'json', on: [], hosts: [], headers: {}, expiring_days: 14 },
    });
  });

  it("keeps a saved block's URL and header values unless they're typed again", () => {
    const form = toForm(saved);
    // Never sent back by the API: left empty, they keep what's saved
    expect(form.url).toBe('');
    expect(form.headers).toEqual([{ name: 'authorization', value: '', saved: true }]);
    const result = fromForm({ ...form, headers: [...form.headers, { name: 'x-team', value: 'payments' }] }, true);
    expect(result).toEqual({
      name: 'ops',
      body: {
        url: '',
        format: 'slack',
        on: ['upstream_down'],
        hosts: ['*.example.com'],
        headers: { authorization: '', 'x-team': 'payments' },
        expiring_days: 14,
      },
    });
  });

  it('says what to fix', () => {
    const form = { ...emptyNotify(), name: 'a/b', url: 'hooks.example.com', expiring_days: '0' };
    expect(fromForm({ ...form, headers: [{ name: 'x-new', value: '' }] }, false)).toEqual({
      errors: [
        'The name can only have letters, digits, -, _ and .',
        'The URL must start with https:// or http://',
        'The header x-new needs a value',
        'Days before expiry must be between 1 and 60',
      ],
    });
    // Also when changing a saved block: only the headers it has keep a value
    const added = { ...toForm(saved), headers: [{ name: 'x-new', value: '' }] };
    expect(fromForm(added, true)).toEqual({ errors: ['The header x-new needs a value'] });
    // A new block needs a URL; a saved one keeps its own
    expect(fromForm({ ...emptyNotify(), name: 'ops' }, false)).toEqual({ errors: ['The URL is required'] });
  });
});
