import { describe, expect, it } from 'vitest';
import { isSection, label, value } from './settings';

describe('settings', () => {
  it('reads names as labels', () => {
    expect(label('shutdown_timeout_secs')).toBe('Shutdown timeout');
    expect(label('dns_provider')).toBe('DNS provider');
    expect(label('tls_min_version')).toBe('TLS min version');
    expect(label('memory_limit_mb')).toBe('Memory limit MB');
    expect(label('lets_encrypt')).toBe("Let's Encrypt");
  });

  it('reads values with their units', () => {
    expect(value('idle_secs', 60)).toBe('60s');
    expect(value('memory_limit_mb', 256)).toBe('256 MB');
    expect(value('enabled', true)).toBe('on');
    expect(value('path', null)).toBe('none');
  });

  it('tells sections from values', () => {
    expect(isSection({ a: 1 })).toBe(true);
    expect(isSection(['a'])).toBe(false);
    expect(isSection(null)).toBe(false);
  });
});
