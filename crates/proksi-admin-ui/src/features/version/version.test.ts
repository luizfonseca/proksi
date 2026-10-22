import { describe, expect, it } from 'vitest';
import { isNewer, newest } from './version';

const release = (tag_name: string, more: { draft?: boolean; prerelease?: boolean } = {}) => ({
  tag_name,
  html_url: `https://github.com/luizfonseca/proksi/releases/tag/${tag_name}`,
  draft: false,
  prerelease: false,
  ...more,
});

describe('new versions', () => {
  it("are Proksi's releases, not its crates'", () => {
    const releases = [
      release('proksi_store-v0.18.0'),
      release('proksi-v0.19.0', { draft: true }),
      release('proksi-v0.18.0-rc.1', { prerelease: true }),
      release('proksi-v0.18.0'),
      release('proksi-v0.17.0'),
    ];
    expect(newest(releases)).toEqual({ version: '0.18.0', url: releases[3].html_url });
    expect(newest([release('proksi_plugins-v0.17.0')])).toBeNull();
    expect(newest({ message: 'API rate limit exceeded' })).toBeNull();
  });

  it('are newer by each number, not as text', () => {
    expect(isNewer('0.18.0', '0.17.0')).toBe(true);
    expect(isNewer('0.17.10', '0.17.9')).toBe(true);
    expect(isNewer('1.0.0', '0.99.3')).toBe(true);
    expect(isNewer('0.17.0', '0.17.0')).toBe(false);
    expect(isNewer('0.16.1', '0.17.0')).toBe(false);
  });
});
