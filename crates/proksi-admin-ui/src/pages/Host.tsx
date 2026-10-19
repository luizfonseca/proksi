import { useState } from 'preact/hooks';
import { type Host as HostData, routes, type TcpBlock, type Upstream } from '../api';
import { Lock, Unlock } from '../components/icons';
import { LineChart } from '../components/Chart';
import { Flow } from '../components/Flow';
import { RequestLog } from '../components/RequestLog';
import { Rules } from '../components/Rules';
import { TcpBlocks } from '../components/Tcp';
import { layout } from '../flow';
import { ms, rate } from '../format';
import { useLive } from '../live';
import { Loaded, ProtocolBadge, SourceBadge } from '../components/State';
import { navigate, onLink } from '../router';
import { useCanWrite } from '../session';
import { useData } from '../useData';

function UpstreamRow({ upstream }: { upstream: Upstream }) {
  // Addresses worth showing: a name's, or a service's
  const resolved = upstream.addresses.filter((address) => address.address !== upstream.upstream);
  return (
    <div class="upstream">
      <span class={`dot ${upstream.healthy ? 'ok' : 'bad'}`} />
      <div>
        <div class="mono">
          {upstream.name ? <span class="name">{upstream.name} → </span> : null}
          {upstream.upstream}
        </div>
        {resolved.length > 0 && !upstream.name && (
          <div class="addresses">
            {resolved.map((address) => (
              <span title={address.healthy ? 'healthy' : 'unhealthy'}>{address.address} </span>
            ))}
          </div>
        )}
        {upstream.addresses.length === 0 && <div class="addresses">not resolved</div>}
      </div>
      {upstream.split && (
        <span class="badge split" title="Traffic split: its share of clients">
          {upstream.split.name} · {upstream.split.percent}%
        </span>
      )}
      <span class="weight">weight {upstream.weight}</span>
    </div>
  );
}

function Actions({ name }: { name: string }) {
  const [error, setError] = useState<string | null>(null);
  async function remove() {
    if (!confirm(`Remove ${name} from the store? Every instance stops serving it.`)) return;
    try {
      await routes.delete(name);
      navigate('/hosts');
    } catch (err) {
      setError(String((err as Error).message ?? err));
    }
  }
  return (
    <div class="actions">
      {error && <span class="error">{error}</span>}
      <button type="button" class="secondary danger" onClick={remove}>Remove</button>
      <a class="button" href={`/hosts/${encodeURIComponent(name)}/edit`} onClick={onLink}>Edit</a>
    </div>
  );
}

/** The host's traffic now (a graph), and since the page was opened (charts) */
function Traffic({ host }: { host: HostData }) {
  const { history, latest } = useLive();
  const stats = latest?.hosts.find((candidate) => candidate.host === host.host);
  const points = history.map((point) => point.hosts[host.host]);
  const read = (value: (point: NonNullable<(typeof points)[number]>) => number | null) =>
    points.map((point) => (point ? value(point) : null));
  return (
    <>
      <section class="panel traffic">
        <div class="chart-head">
          <h3>Traffic</h3>
          <div class="legend">
            <span><i style={{ background: 'var(--ok)' }} />flowing</span>
            <span><i style={{ background: 'var(--warn)' }} />some errors</span>
            <span><i style={{ background: 'var(--bad)' }} />failing or unhealthy</span>
            <span>over the last 5 seconds</span>
          </div>
        </div>
        <Flow flow={layout(host, stats, latest?.window_seconds)} />
      </section>
      <div class="host-charts">
        <LineChart
          title="Requests"
          span={300}
          format={rate}
          series={[
            { name: 'all', color: 'var(--ok)', values: read((point) => point.requests) },
            { name: '5xx', color: 'var(--bad)', values: read((point) => point.errors) },
          ]}
        />
        <LineChart
          title="Latency"
          span={300}
          format={ms}
          series={[
            { name: 'p50', color: 'var(--chart-1)', values: read((point) => point.p50) },
            { name: 'p99', color: 'var(--chart-3)', values: read((point) => point.p99) },
          ]}
        />
      </div>
      <RequestLog host={host.host} />
    </>
  );
}

export function Host({ name }: { name: string }) {
  const [data] = useData<HostData[]>('/hosts');
  const [tcp] = useData<TcpBlock[]>('/tcp', 10000);
  const canWrite = useCanWrite();
  return (
    <Loaded data={data}>
      {(hosts) => {
        const host = hosts.find((candidate) => candidate.host === name);
        const blocks = tcp.state === 'ready' ? tcp.value.filter((block) => block.host === host?.host) : [];
        if (!host) {
          return (
            <div class="panel empty">
              <strong>No host named {name}</strong>
              <a href="/hosts" onClick={onLink}>Back to hosts</a>
            </div>
          );
        }
        return (
          <>
            <div class="crumbs">
              <a href="/hosts" onClick={onLink}>Hosts</a> / {host.host}
            </div>
            <div class="page-head">
              <div>
                <h1>{host.host}</h1>
                <div class="meta" style={{ marginTop: 8 }}>
                  <SourceBadge source={host.source} />
                  {host.locations.length > 0 && <ProtocolBadge protocol="http" />}
                  {blocks.length > 0 && <ProtocolBadge protocol="tcp" />}
                  <span class="badge">
                    {host.tls ? <Lock /> : <Unlock />} {host.tls ? 'HTTPS' : 'Plain HTTP'}
                  </span>
                  {host.listen.map((address) => <span class="chip">{address}</span>)}
                </div>
              </div>
              {canWrite && host.source === 'store' && <Actions name={host.host} />}
            </div>
            <Traffic host={host} />
            {host.rules.length > 0 && (
              <section class="panel host-rules">
                <h3>Host rules <small>every request, before its location is chosen</small></h3>
                <Rules rules={host.rules} />
              </section>
            )}
            <h2 class="section-title">
              Locations
              <ProtocolBadge protocol="http" />
            </h2>
            <div class="locations">
              {host.locations.map((location) => (
                <div class="panel" id={encodeURIComponent(`location ${location.location}`)}>
                  <div class="location-head">
                    <span class="path">{location.location}</span>
                    <span class="badge" style={{ marginLeft: 'auto' }}>
                      {location.upstreams.length} upstream{location.upstreams.length === 1 ? '' : 's'}
                    </span>
                  </div>
                  <div class="location-body">
                    <section>
                      <h3>Upstreams</h3>
                      {location.upstreams.length === 0 ? (
                        <span class="none">Answered by Proksi (no upstream)</span>
                      ) : (
                        location.upstreams.map((upstream) => <UpstreamRow upstream={upstream} />)
                      )}
                    </section>
                    <section>
                      <h3>Plugins</h3>
                      <div class="chips">
                        {location.blocks.length === 0 ? <span class="none">None</span> : location.blocks.map((block) => <span class="chip">{block}</span>)}
                      </div>
                    </section>
                    {location.rules.length > 0 && (
                      <section class="location-rules">
                        <h3>Rules</h3>
                        <Rules rules={location.rules} />
                      </section>
                    )}
                  </div>
                </div>
              ))}
            </div>
            <TcpBlocks blocks={blocks} />
          </>
        );
      }}
    </Loaded>
  );
}
