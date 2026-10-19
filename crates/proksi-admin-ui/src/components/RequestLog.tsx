import { useState } from 'preact/hooks';
import { ms } from '../format';
import { useRequests } from '../requests';
import { navigate } from '../router';

/** The latest requests, of every host or of `host` */
export function RequestLog({ host }: { host?: string }) {
  // Off until asked for: Proksi builds nothing for it until then
  const [paused, setPaused] = useState(true);
  const [held, setHeld] = useState(false);
  const { requests, skipped } = useRequests(paused, held, host);
  return (
    <section class="panel requests">
      <div class="chart-head">
        <h3>Latest requests</h3>
        <div class="legend">
          {held && !paused && <span>Held while the pointer is on the list</span>}
          {skipped > 0 && <span>{skipped} not shown last second (more than 100)</span>}
          <button type="button" class={paused && !requests.length ? 'primary' : 'secondary'} onClick={() => setPaused(!paused)}>
            {paused ? (requests.length ? 'Resume' : 'Watch requests') : 'Pause'}
          </button>
        </div>
      </div>
      {requests.length === 0 ? (
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
              {requests.map((served) => (
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

