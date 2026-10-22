import { useEffect, useState } from 'preact/hooks';
import { Conflict, type Expect, type HistoryEntry, routes } from '../api';
import { useCanWrite } from '../api/session';
import { Compare } from '../features/history/Compare';
import { activeVersion } from '../features/history/versions';
import { Alert } from '../components/ui/Alert';
import { Badge } from '../components/ui/Badge';
import { Button } from '../components/ui/Button';
import { Checkbox } from '../components/ui/Form';
import { Empty, None, PageHead, Panel } from '../components/ui/Page';
import { Cell, Row, Table } from '../components/ui/Table';
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

  if (!changes) return error ? <Alert>{error}</Alert> : null;
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
      <PageHead
        crumbs={
          <ul>
            <li><a href="/hosts" onClick={onLink}>Hosts</a></li>
            <li>{current === null ? name : <a href={`/hosts/${encodeURIComponent(name)}`} onClick={onLink}>{name}</a>}</li>
            <li>history</li>
          </ul>
        }
        title={name}
        description={
          changes.length === 0
            ? 'In the store, saved before versions were kept.'
            : active === null
              ? 'Deleted. Restore a version to serve it again.'
              : `Serving version ${active}. Select versions to compare them.`
        }
      />
      {error && <Alert class="mb-4">{error}</Alert>}

      {changes.length === 0 ? (
        <Panel>
          <Empty title="No changes recorded yet">
            The history starts with the next change to this host. Changes made before Proksi kept versions aren't recorded.
          </Empty>
        </Panel>
      ) : (
        <Panel class="overflow-hidden">
          <Table columns={[{ label: 'Compare', class: 'w-20' }, 'Change', 'By', 'When', '']}>
            {changes.map((entry) => {
              const version = versionOf(entry);
              const at = new Date(entry.at * 1000);
              return (
                <Row>
                  <Cell>
                    {version !== null && (
                      <Checkbox
                        hidden
                        label={`Compare version ${version}`}
                        checked={selected.includes(version)}
                        onChange={() => toggle(version)}
                      />
                    )}
                  </Cell>
                  <Cell class="font-medium">
                    {describe(entry)}
                    {version !== null && version === active && (
                      <Badge tone="success" class="ml-2">
                        serving
                      </Badge>
                    )}
                  </Cell>
                  <Cell>{entry.author ?? <None>unknown</None>}</Cell>
                  <Cell class="text-base-content/60">
                    <span title={at.toISOString()}>{at.toLocaleString()}</span>
                  </Cell>
                  <Cell class="text-right">
                    {canWrite && version !== null && version !== active && (
                      <Button disabled={busy} onClick={() => rollback(version)}>
                        {active === null ? 'Restore' : 'Roll back to this'}
                      </Button>
                    )}
                  </Cell>
                </Row>
              );
            })}
          </Table>
        </Panel>
      )}

      {pair && (
        <Panel class="mt-4 pb-3.5" title={`Version ${pair[0]} → version ${pair[1]}`}>
          <div class="px-4">
            <Compare name={name} before={pair[0]} after={pair[1]} />
          </div>
        </Panel>
      )}
    </>
  );
}
