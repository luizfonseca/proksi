import type { Host as HostData } from '../api';
import { Lock, Unlock } from '../components/icons';
import { Loaded, SourceBadge } from '../components/State';
import { onLink } from '../router';
import { useData } from '../useData';

export function Host({ name }: { name: string }) {
  const [data] = useData<HostData[]>('/hosts');
  return (
    <Loaded data={data}>
      {(hosts) => {
        const host = hosts.find((candidate) => candidate.host === name);
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
                  <span class="badge">
                    {host.tls ? <Lock /> : <Unlock />} {host.tls ? 'HTTPS' : 'Plain HTTP'}
                  </span>
                  {host.listen.map((address) => <span class="chip">{address}</span>)}
                </div>
              </div>
            </div>
            <div class="locations">
              {host.locations.map((location) => (
                <div class="panel">
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
                        location.upstreams.map((upstream) => (
                          <div class="upstream">
                            <span class={`dot ${upstream.healthy ? 'ok' : 'bad'}`} />
                            <span class="mono">{upstream.upstream}</span>
                            <span class="weight">weight {upstream.weight}</span>
                          </div>
                        ))
                      )}
                    </section>
                    <section>
                      <h3>Plugins</h3>
                      <div class="chips">
                        {location.blocks.length === 0 ? <span class="none">None</span> : location.blocks.map((block) => <span class="chip">{block}</span>)}
                      </div>
                    </section>
                  </div>
                </div>
              ))}
            </div>
          </>
        );
      }}
    </Loaded>
  );
}
