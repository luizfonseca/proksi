import { useState } from 'preact/hooks';
import type { Host, TcpBlock } from '../api';
import { Lock, Search, Unlock } from '../components/icons';
import { Loaded, ProtocolBadge, SourceBadge } from '../components/State';
import { navigate, onLink } from '../router';
import { useCanWrite } from '../session';
import { useData } from '../useData';

function Health({ host }: { host: Host }) {
  const upstreams = host.locations.flatMap((location) => location.upstreams);
  if (upstreams.length === 0) return <span class="none">no upstreams</span>;
  const healthy = upstreams.filter((upstream) => upstream.healthy).length;
  return (
    <span class="health">
      <span class="dots">
        {upstreams.slice(0, 8).map((upstream) => (
          <span class={`dot ${upstream.healthy ? 'ok' : 'bad'}`} title={upstream.upstream} />
        ))}
      </span>
      {healthy}/{upstreams.length}
    </span>
  );
}

function Summary({ hosts }: { hosts: Host[] }) {
  const upstreams = hosts.flatMap((host) => host.locations.flatMap((location) => location.upstreams));
  const unhealthy = upstreams.filter((upstream) => !upstream.healthy).length;
  const locations = hosts.reduce((count, host) => count + host.locations.length, 0);
  return (
    <div class="stats">
      <div class="panel stat"><div class="label">Hosts</div><div class="value">{hosts.length}</div></div>
      <div class="panel stat"><div class="label">Locations</div><div class="value">{locations}</div></div>
      <div class="panel stat"><div class="label">Upstreams</div><div class="value">{upstreams.length}</div></div>
      <div class="panel stat">
        <div class="label">Unhealthy upstreams</div>
        <div class="value" style={{ color: unhealthy ? 'var(--bad)' : undefined }}>{unhealthy}</div>
      </div>
    </div>
  );
}

export function Hosts() {
  const [data] = useData<Host[]>('/hosts');
  const [tcp] = useData<TcpBlock[]>('/tcp', 10000);
  const tcpHosts = new Set(tcp.state === 'ready' ? tcp.value.map((block) => block.host) : []);
  const [query, setQuery] = useState('');
  const [source, setSource] = useState('all');
  const canWrite = useCanWrite();

  return (
    <>
      <div class="page-head">
        <div>
          <h1>Hosts</h1>
          <p>Every host Proksi serves, from every source.</p>
        </div>
        {canWrite && (
          <div class="actions">
            <a class="button primary" href="/hosts/new" onClick={onLink}>New host</a>
          </div>
        )}
      </div>
      <Loaded data={data}>
        {(hosts) => {
          const sources = ['all', ...new Set(hosts.map((host) => host.source ?? 'unknown'))];
          const shown = hosts.filter(
            (host) =>
              host.host.includes(query.trim().toLowerCase()) &&
              (source === 'all' || (host.source ?? 'unknown') === source),
          );
          return (
            <>
              <Summary hosts={hosts} />
              <div class="toolbar">
                <label class="search">
                  <Search />
                  <input placeholder="Search hosts" value={query} onInput={(e) => setQuery(e.currentTarget.value)} />
                </label>
                {sources.length > 2 && (
                  <div class="segmented">
                    {sources.map((name) => (
                      <button aria-pressed={source === name} onClick={() => setSource(name)}>{name}</button>
                    ))}
                  </div>
                )}
              </div>
              <div class="panel">
                {shown.length === 0 ? (
                  <div class="empty">
                    <strong>{hosts.length === 0 ? 'No hosts yet' : 'No hosts match'}</strong>
                    {hosts.length === 0 ? 'Add a host block to the configuration file.' : 'Try another search.'}
                  </div>
                ) : (
                  <table>
                    <thead>
                      <tr>
                        <th>Host</th>
                        <th>Source</th>
                        <th class="hide-sm">Locations</th>
                        <th>Upstreams</th>
                        <th class="hide-sm">Plugins</th>
                      </tr>
                    </thead>
                    <tbody>
                      {shown.map((host) => {
                        const blocks = [...new Set(host.locations.flatMap((location) => location.blocks))];
                        return (
                          <tr class="link" onClick={() => navigate(`/hosts/${encodeURIComponent(host.host)}`)}>
                            <td>
                              <span class="host-name">
                                {host.host}
                                {host.tls ? (
                                  <span class="tls" title="HTTPS"><Lock /></span>
                                ) : (
                                  <span class="plain" title="Plain HTTP: no TLS"><Unlock /></span>
                                )}
                                {host.locations.length > 0 && <ProtocolBadge protocol="http" />}
                                {tcpHosts.has(host.host) && <ProtocolBadge protocol="tcp" />}
                              </span>
                            </td>
                            <td><SourceBadge source={host.source} /></td>
                            <td class="num hide-sm">{host.locations.length}</td>
                            <td><Health host={host} /></td>
                            <td class="hide-sm">
                              <div class="chips">
                                {blocks.length === 0 ? <span class="none">none</span> : blocks.map((block) => <span class="chip">{block}</span>)}
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                )}
              </div>
            </>
          );
        }}
      </Loaded>
    </>
  );
}
