import type { TcpBlock } from '../../api';
import { bytes, ms, rate } from '../../lib/format';
import { useLive } from '../live/live';
import { onLink } from '../../router';
import { ProtocolBadge } from '../../components/ui/State';
import { Sparkline } from '../../components/ui/Chart';

const TLS = { none: 'plain TCP', terminate: 'TLS terminated', passthrough: 'TLS passthrough' };

/** The `tcp` blocks, with their connections over the last 5 seconds */
export function TcpBlocks({ blocks, withHost }: { blocks: TcpBlock[]; withHost?: boolean }) {
  const { history, latest } = useLive();
  if (blocks.length === 0) return null;
  return (
    <section class="panel tcp">
      <div class="chart-head">
        <h3>
          Blocks
          <ProtocolBadge protocol="tcp" />
        </h3>
        <div class="legend">
          <span>connections over the last 5 seconds</span>
        </div>
      </div>
      <div class="table-wide">
        <table class="tcp-blocks">
          <thead>
            <tr>
              <th>Block</th>
              <th>Upstreams</th>
              <th>New</th>
              <th>Open</th>
              <th>Traffic</th>
              <th>Failed</th>
              <th title="How long the connections closed over the last 5 seconds stayed open">Open for (p50 · p90 · p99)</th>
              <th class="trend">Open, last 2 minutes</th>
            </tr>
          </thead>
          <tbody>
            {blocks.map((block) => {
              const stats = latest?.tcp?.find((candidate) => candidate.block === block.block);
              const failed = stats?.upstream_errors ?? 0;
              const healthy = block.upstreams.length > 0 && block.upstreams.every((upstream) => upstream.healthy);
              return (
                <tr>
                  <td>
                    <div class="name">
                      {block.name}
                      {withHost && block.host && (
                        <>
                          {' in '}
                          <a href={`/hosts/${encodeURIComponent(block.host)}`} onClick={onLink}>
                            {block.host}
                          </a>
                        </>
                      )}
                    </div>
                    <div class="addresses">
                      {block.listen} · {TLS[block.tls]}
                      {block.alpn.length > 0 && ` · ${block.alpn.join(', ')}`}
                    </div>
                  </td>
                  <td>
                    {block.upstreams.map((upstream) => (
                      <div class="mono" title={upstream.healthy ? 'healthy' : 'unhealthy'}>
                        <span class={`dot ${upstream.healthy ? 'ok' : 'bad'}`} /> {upstream.name ?? upstream.upstream}
                      </div>
                    ))}
                  </td>
                  <td class="mono">{stats ? rate(stats.connections_per_second) : '—'}</td>
                  <td class="mono">{stats ? stats.active : '—'}</td>
                  <td class="mono">
                    {stats ? (
                      <>
                        ↑ {bytes(stats.bytes_in_per_second)}/s
                        <br />↓ {bytes(stats.bytes_out_per_second)}/s
                      </>
                    ) : (
                      '—'
                    )}
                  </td>
                  <td>
                    <span class={`badge ${failed > 0 ? 'bad' : 'ok'}`}>{failed}</span>
                  </td>
                  <td class="mono">
                  {stats ? `${ms(stats.duration_ms.p50)} · ${ms(stats.duration_ms.p90)} · ${ms(stats.duration_ms.p99)}` : '—'}
                </td>
                  <td class="trend">
                    <Sparkline values={history.map((point) => point.tcp?.[block.block]?.active ?? 0)} color={failed > 0 || !healthy ? 'var(--warn)' : 'var(--ok)'} />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}
