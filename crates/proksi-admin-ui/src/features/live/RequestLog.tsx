import { useState } from 'preact/hooks';
import { ms } from '../../lib/format';
import { matches, useRequests } from './requests';
import { Badge } from '../../components/ui/Badge';
import { Button } from '../../components/ui/Button';
import { SearchInput } from '../../components/ui/Form';
import { Empty, Panel } from '../../components/ui/Page';
import { Stack } from '../../components/ui/Stack';
import { Cell, Row, Table } from '../../components/ui/Table';
import { navigate } from '../../router';

/** Rows drawn at once: the newest matching ones (up to `KEPT` are kept to search) */
const DRAWN = 200;

/** The latest requests, of every host or of `host` */
export function RequestLog({ host }: { host?: string }) {
  // Off until asked for: Proksi builds nothing for it until then
  const [paused, setPaused] = useState(true);
  const [held, setHeld] = useState(false);
  const [filter, setFilter] = useState('');
  const { requests, skipped } = useRequests(paused, held, host);
  const shown = filter.trim() ? requests.filter((served) => matches(served, filter.trim())) : requests;
  return (
    <Panel
      class="mb-3"
      title="Latest requests"
      aside={
        <Stack gap={3} wrap justify="end" class="flex-1 text-xs text-base-content/60">
          {requests.length > 0 && <SearchInput class="w-full max-w-60" placeholder="Filter: path, 5xx, client…" value={filter} onInput={setFilter} />}
          {held && !paused && <span>Held while the pointer is on the list</span>}
          {!paused && <span>up to 100 a second</span>}
          {skipped > 0 && <span>{skipped} missed last second</span>}
          <Button variant={paused && !requests.length ? 'primary' : 'secondary'} onClick={() => setPaused(!paused)}>
            {paused ? (requests.length ? 'Resume' : 'Watch requests') : 'Pause'}
          </Button>
        </Stack>
      }
    >
      {requests.length > 0 && shown.length === 0 ? (
        <Empty compact title="No requests match">
          {requests.length} kept: try another filter.
        </Empty>
      ) : requests.length === 0 ? (
        <Empty compact title={paused ? 'Not watching' : 'Waiting for requests…'}>
          {paused
            ? `Watch requests to see each one${host ? ` to ${host}` : ''} as it's served, while this page is open.`
            : `Requests${host ? ` to ${host}` : ''} appear here as they're served.`}
        </Empty>
      ) : (
        <div class="font-mono text-xs [&_th]:font-sans" onMouseEnter={() => setHeld(true)} onMouseLeave={() => setHeld(false)}>
          <Table height={420} columns={['Time', 'Status', 'Request', 'Duration', 'Client', 'Upstream']}>
            {shown.slice(0, DRAWN).map((served) => (
              <Row onClick={() => !host && served.host && navigate(`/hosts/${encodeURIComponent(served.host)}`)}>
                <Cell class="text-base-content/45">{new Date(served.time).toLocaleTimeString()}</Cell>
                <Cell>
                  <Badge class="font-sans" tone={served.status >= 500 ? 'error' : served.status >= 400 ? 'warning' : 'success'}>
                    {served.status}
                  </Badge>
                </Cell>
                <Cell>
                  <b class="mr-1.5 font-semibold">{served.method}</b> {host ? null : served.host}
                  <span class={host ? undefined : 'text-base-content/45'}>{served.path}</span>
                </Cell>
                <Cell>{ms(served.duration_ms)}</Cell>
                <Cell class="text-base-content/45">{served.client_ip}</Cell>
                <Cell class="text-base-content/45">{served.upstream ?? '—'}</Cell>
              </Row>
            ))}
          </Table>
        </div>
      )}
    </Panel>
  );
}
