import { useState } from 'preact/hooks';
import { ms } from '../../lib/format';
import { matches, useRequests } from './requests';
import { Search } from '../../components/ui/icons';
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
    <section class="panel requests">
      <div class="chart-head">
        <h3>Latest requests</h3>
        <div class="legend">
          {requests.length > 0 && (
            <label class="search">
              <Search />
              <input placeholder="Filter: path, 5xx, client…" value={filter} onInput={(event) => setFilter(event.currentTarget.value)} />
            </label>
          )}
          {held && !paused && <span>Held while the pointer is on the list</span>}
          {!paused && <span>up to 100 a second</span>}
          {skipped > 0 && <span>{skipped} missed last second</span>}
          <button type="button" class={paused && !requests.length ? 'primary' : 'secondary'} onClick={() => setPaused(!paused)}>
            {paused ? (requests.length ? 'Resume' : 'Watch requests') : 'Pause'}
          </button>
        </div>
      </div>
      {requests.length > 0 && shown.length === 0 ? (
        <div class="empty">
          <strong>No requests match</strong>
          {requests.length} kept: try another filter.
        </div>
      ) : requests.length === 0 ? (
        <div class="empty">
          <strong>{paused ? 'Not watching' : 'Waiting for requests…'}</strong>
          {paused
            ? `Watch requests to see each one${host ? ` to ${host}` : ''} as it's served, while this page is open.`
            : `Requests${host ? ` to ${host}` : ''} appear here as they're served.`}
        </div>
      ) : (
        <div class="table-scroll" onMouseEnter={() => setHeld(true)} onMouseLeave={() => setHeld(false)}>
          <table>
            <thead>
              <tr>
                <th>Time</th>
                <th>Status</th>
                <th>Request</th>
                <th>Duration</th>
                <th>Client</th>
                <th>Upstream</th>
              </tr>
            </thead>
            <tbody>
              {shown.slice(0, DRAWN).map((served) => (
                <tr onClick={() => !host && served.host && navigate(`/hosts/${encodeURIComponent(served.host)}`)}>
                  <td class="mono muted">{new Date(served.time).toLocaleTimeString()}</td>
                  <td>
                    <span class={`badge ${served.status >= 500 ? 'bad' : served.status >= 400 ? 'warn' : 'ok'}`}>{served.status}</span>
                  </td>
                  <td class="mono">
                    <b>{served.method}</b> {host ? null : served.host}
                    <span class={host ? undefined : 'muted'}>{served.path}</span>
                  </td>
                  <td class="mono">{ms(served.duration_ms)}</td>
                  <td class="mono muted">{served.client_ip}</td>
                  <td class="mono muted">{served.upstream ?? '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

