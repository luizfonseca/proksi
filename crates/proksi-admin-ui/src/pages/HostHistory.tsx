import { useEffect, useState } from 'preact/hooks';
import { Conflict, type Expect, type HistoryEntry, routes } from '../api';
import { useCanWrite } from '../api/session';
import { Compare } from '../features/history/Compare';
import { activeVersion } from '../features/history/versions';
import { onLink } from '../router';

/** The versions a change can be compared with or rolled back to */
function versionOf(entry: HistoryEntry): number | null {
  if (entry.change === 'saved') return entry.revision;
  return null;
}

function describe(entry: HistoryEntry): string {
  if (entry.change === 'saved') return `Saved version ${entry.revision}`;
  if (entry.change === 'deleted') return 'Deleted';
  return `Rolled back to version ${entry.to}`;
}

/** The changes to a host in the store, newest first: compare versions, roll back to one */
export function HostHistory({ name }: { name: string }) {
  const [changes, setChanges] = useState<HistoryEntry[] | null>(null);
  // The host's active version as read: what a rollback is made from (null: not in the store)
  const [current, setCurrent] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<number[]>([]);
  const [busy, setBusy] = useState(false);
  const canWrite = useCanWrite();

  async function load() {
    setError(null);
    try {
      const [history, host] = await Promise.all([routes.history(name), routes.get(name).catch(() => null)]);
      setChanges(history.changes);
      setCurrent(host?.version ?? null);
    } catch (err) {
      setError(String((err as Error).message ?? err));
    }
  }
  useEffect(() => {
    load();
  }, [name]);

  async function rollback(version: number) {
    const what = active === null ? `Restore ${name} as version ${version}` : `Roll ${name} back to version ${version}`;
    if (!confirm(`${what}? Every instance serves it within a few seconds.`)) return;
    const expect: Expect = current === null ? 'absent' : { version: current };
    setBusy(true);
    try {
      await routes.rollback(name, version, expect);
      setSelected([]);
      await load();
    } catch (err) {
      setError(
        err instanceof Conflict
          ? 'Someone else changed this host since you opened this page. Reload to see their change.'
          : String((err as Error).message ?? err),
      );
    } finally {
      setBusy(false);
    }
  }

  function toggle(version: number) {
    setSelected((current) =>
      current.includes(version) ? current.filter((other) => other !== version) : [...current, version].slice(-2),
    );
  }

  if (!changes) return error ? <div class="error">{error}</div> : null;
  const active = activeVersion(changes);
  // One version: compared with the active one. Two: the older one first.
  const pair =
    selected.length === 2
      ? [Math.min(...selected), Math.max(...selected)]
      : selected.length === 1 && active !== null && selected[0] !== active
        ? [Math.min(selected[0], active), Math.max(selected[0], active)]
        : null;

  return (
    <>
      <div class="crumbs">
        <a href="/hosts" onClick={onLink}>Hosts</a> /{' '}
        {active === null ? name : <a href={`/hosts/${encodeURIComponent(name)}`} onClick={onLink}>{name}</a>} / history
      </div>
      <div class="page-head">
        <div>
          <h1>{name}</h1>
          <p>
            {active === null
              ? 'Deleted. Restore a version to serve it again.'
              : `Serving version ${active}. Select versions to compare them.`}
          </p>
        </div>
      </div>
      {error && <div class="error" role="alert">{error}</div>}

      <div class="panel">
        <table class="history">
          <thead>
            <tr>
              <th>Compare</th>
              <th>Change</th>
              <th>By</th>
              <th>When</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {changes.map((entry) => {
              const version = versionOf(entry);
              return (
                <tr>
                  <td>
                    {version !== null && (
                      <input
                        type="checkbox"
                        aria-label={`Compare version ${version}`}
                        checked={selected.includes(version)}
                        onChange={() => toggle(version)}
                      />
                    )}
                  </td>
                  <td>
                    {describe(entry)}
                    {version !== null && version === active && <span class="badge" style={{ marginLeft: 8 }}>active</span>}
                  </td>
                  <td>{entry.author ?? <span class="none">unknown</span>}</td>
                  <td title={new Date(entry.at * 1000).toISOString()}>{new Date(entry.at * 1000).toLocaleString()}</td>
                  <td class="num">
                    {canWrite && version !== null && version !== active && (
                      <button type="button" class="secondary" disabled={busy} onClick={() => rollback(version)}>
                        {active === null ? 'Restore' : 'Roll back to this'}
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {pair && (
        <section class="panel compare">
          <h3>
            Version {pair[0]} → version {pair[1]}
          </h3>
          <Compare name={name} before={pair[0]} after={pair[1]} />
        </section>
      )}
    </>
  );
}
