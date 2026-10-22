import { useEffect, useState } from 'preact/hooks';

// The newest release of Proksi, asked of GitHub by the browser once each time the UI is opened.
// Proksi itself never asks: without access to GitHub, nothing is shown.

const RELEASES = 'https://api.github.com/repos/luizfonseca/proksi/releases?per_page=20';

/** Proksi's own releases: the crates are released apart (`proksi_store-v0.17.0`) */
const TAG = /^proksi-v(\d+\.\d+\.\d+)$/;

export type Release = { version: string; url: string };

type GithubRelease = { tag_name: string; html_url: string; draft: boolean; prerelease: boolean };

/** The newest of Proksi's releases among GitHub's answer (newest first), if any */
export function newest(answer: unknown): Release | null {
  if (!Array.isArray(answer)) return null;
  for (const release of answer as GithubRelease[]) {
    const version = TAG.exec(release.tag_name)?.[1];
    if (version && !release.draft && !release.prerelease) return { version, url: release.html_url };
  }
  return null;
}

/** Whether `version` comes after `current`, number by number */
export function isNewer(version: string, current: string): boolean {
  const a = version.split('.').map(Number);
  const b = current.split('.').map(Number);
  for (let at = 0; at < 3; at++) {
    if (a[at] !== b[at]) return a[at] > b[at];
  }
  return false;
}

/** Asked once while the page is open */
let asked: Promise<Release | null> | null = null;

function ask(): Promise<Release | null> {
  asked ??= fetch(RELEASES, { headers: { accept: 'application/vnd.github+json' } })
    .then((response) => (response.ok ? response.json() : null))
    .then(newest)
    .catch(() => null);
  return asked;
}

/** A release newer than `current`, once known */
export function useNewRelease(current: string | undefined): Release | null {
  const [latest, setLatest] = useState<Release | null>(null);
  useEffect(() => {
    ask().then(setLatest);
  }, []);
  return latest && current && isNewer(latest.version, current) ? latest : null;
}
