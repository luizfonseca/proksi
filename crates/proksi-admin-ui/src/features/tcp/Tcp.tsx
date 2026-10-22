import type { ComponentChildren } from 'preact';
import type { TcpBlock } from '../../api';
import { bytes, ms, rate } from '../../lib/format';
import { useLive } from '../live/live';
import { Badge, ProtocolBadge, Status } from '../../components/ui/Badge';
import { Legend, Sparkline } from '../../components/ui/Chart';
import { Hint, HostLink, Mono, Panel } from '../../components/ui/Page';
import { Stack } from '../../components/ui/Stack';
import { Cell, Row, Table } from '../../components/ui/Table';

const TLS = { none: 'plain TCP', terminate: 'TLS terminated', passthrough: 'TLS passthrough' };

/** A number or address, in monospace on one line */
function Figure({ phone, children }: { phone?: boolean; children: ComponentChildren }) {
  return (
    <Cell phone={phone}>
      <Mono class="whitespace-nowrap">{children}</Mono>
    </Cell>
  );
}

/** The `tcp` blocks, with their connections over the last 5 seconds */
export function TcpBlocks({ blocks, withHost }: { blocks: TcpBlock[]; withHost?: boolean }) {
  const { history, latest } = useLive();
  if (blocks.length === 0) return null;
  return (
    <Panel
      class="mb-3"
      title={
        <Stack>
          Blocks
          <ProtocolBadge protocol="tcp" />
        </Stack>
      }
      aside={
        <Legend>
          <span>connections over the last 5 seconds</span>
        </Legend>
      }
    >
      <Table
        columns={[
          'Block',
          { label: 'Upstreams', phone: false },
          'New',
          'Open',
          { label: 'Traffic', phone: false },
          'Failed',
          {
            label: <span title="How long the connections closed over the last 5 seconds stayed open">Open for (p50 · p90 · p99)</span>,
            phone: false,
          },
          { label: 'Open, last 2 minutes', phone: false, class: 'w-[220px]' },
        ]}
      >
        {blocks.map((block) => {
          const stats = latest?.tcp?.find((candidate) => candidate.block === block.block);
          const failed = stats?.upstream_errors ?? 0;
          const healthy = block.upstreams.length > 0 && block.upstreams.every((upstream) => upstream.healthy);
          return (
            <Row top>
              <Cell>
                <div class="font-semibold">
                  {block.name}
                  {withHost && block.host && (
                    <>
                      {' in '}
                      <HostLink host={block.host} />
                    </>
                  )}
                </div>
                <Hint class="block font-mono whitespace-nowrap">
                  {block.listen} · {TLS[block.tls]}
                  {block.alpn.length > 0 && ` · ${block.alpn.join(', ')}`}
                </Hint>
              </Cell>
              <Cell phone={false}>
                {block.upstreams.map((upstream) => (
                  <Stack gap={1.5}>
                    <Status tone={upstream.healthy ? 'success' : 'error'} title={upstream.healthy ? 'healthy' : 'unhealthy'} />
                    <Mono class="whitespace-nowrap">{upstream.name ?? upstream.upstream}</Mono>
                  </Stack>
                ))}
              </Cell>
              <Figure>{stats ? rate(stats.connections_per_second) : '—'}</Figure>
              <Figure>{stats ? stats.active : '—'}</Figure>
              <Figure phone={false}>
                {stats ? (
                  <>
                    ↑ {bytes(stats.bytes_in_per_second)}/s
                    <br />↓ {bytes(stats.bytes_out_per_second)}/s
                  </>
                ) : (
                  '—'
                )}
              </Figure>
              <Cell>
                <Badge tone={failed > 0 ? 'error' : 'success'}>{failed}</Badge>
              </Cell>
              <Figure phone={false}>
                {stats ? `${ms(stats.duration_ms.p50)} · ${ms(stats.duration_ms.p90)} · ${ms(stats.duration_ms.p99)}` : '—'}
              </Figure>
              <Cell phone={false} class="w-[220px] pt-2.5 pb-0">
                <Sparkline
                  class="block h-7"
                  values={history.map((point) => point.tcp?.[block.block]?.active ?? 0)}
                  color={failed > 0 || !healthy ? 'var(--color-warning)' : 'var(--color-success)'}
                />
              </Cell>
            </Row>
          );
        })}
      </Table>
    </Panel>
  );
}
