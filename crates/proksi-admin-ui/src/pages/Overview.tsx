import { useState } from 'preact/hooks';
import type { Status, TcpBlock } from '../api';
import { Legend, LegendValue, LineChart, Sparkline } from '../components/ui/Chart';
import { Badge, ProtocolBadge, type Tone } from '../components/ui/Badge';
import { Segmented } from '../components/ui/Form';
import { Empty, HostLink, Mono, PageHead, PageSection, Panel } from '../components/ui/Page';
import { Grid, Stack } from '../components/ui/Stack';
import { Stat, Stats as Cards } from '../components/ui/Stats';
import { Cell, Row, Table } from '../components/ui/Table';
import { RequestLog } from '../features/live/RequestLog';
import { Loaded } from '../components/ui/State';
import { TcpBlocks } from '../features/tcp/Tcp';
import { bytes, ms, percent, rate } from '../lib/format';
import { activeHosts, type Point, type Stats, useLive } from '../features/live/live';
import { useData } from '../api/useData';
import { Time } from '../components/ui/Time';
import { useTween } from '../lib/motion';

const SPANS = [
  { label: '1m', points: 60 },
  { label: '5m', points: 300 },
  { label: '15m', points: 900 },
];

const COLORS = {
  '2xx': 'var(--color-success)',
  '3xx': 'var(--color-info)',
  '4xx': 'var(--color-warning)',
  '5xx': 'var(--color-error)',
  p50: 'var(--chart-1)',
  p90: 'var(--chart-2)',
  p99: 'var(--chart-3)',
};

/** A number, with a line of its latest values across the bottom */
/** A number as it changes: rolling to each new value */
function Rolling({ value, format }: { value: number | null; format: (value: number) => string }) {
  const shown = useTween(value);
  return <>{shown === null ? '—' : format(shown)}</>;
}

function Card({
  label,
  value,
  format,
  detail,
  values,
  color,
}: {
  label: string;
  value: number | null;
  format: (value: number) => string;
  detail?: string;
  values: (number | null)[];
  color: string;
}) {
  return (
    <Stat class="relative content-start overflow-hidden pb-11" title={label} value={<Rolling value={value} format={format} />} detail={detail}>
      <Sparkline values={values} color={color} />
    </Stat>
  );
}

const take = (history: Point[], read: (point: Point) => number | null) => history.map(read);

/** A rate of errors, colored by how bad it is */
const tone = (share: number): Tone => (share >= 0.25 ? 'error' : share > 0 ? 'warning' : 'success');

/** The hosts being accessed now (the last 5 seconds), busiest first, then the ones accessed since the page opened */
function ActiveHosts({ latest, history }: { latest: Stats | null; history: Point[] }) {
  const hosts = activeHosts(latest, history);
  const listed = new Set(hosts.map(({ host }) => host));
  const idle = (latest?.hosts ?? []).filter(({ host }) => !listed.has(host)).length;
  return (
    <Panel
      title={
        <Stack>
          Hosts
          <ProtocolBadge protocol="http" />
        </Stack>
      }
      aside={
        <Legend>
          <span>over the last 5 seconds, and since this page opened{idle > 0 ? ` · ${idle} without requests` : ''}</span>
        </Legend>
      }
    >
      {hosts.length === 0 ? (
        <Empty title={latest ? 'No requests' : 'Waiting for data…'}>Hosts appear here once they're accessed, and stay while this page is open.</Empty>
      ) : (
        <Table columns={['Host', 'Requests', '5xx', 'p99', { label: 'Last 2 minutes', phone: false, class: 'w-[220px]' }]}>
          {hosts.map(({ host, point, lastSeen }) => {
            const errors = point.requests ? point.errors / point.requests : 0;
            return (
              <Row>
                <Cell>
                  <HostLink host={host} quiet={point.requests === 0} />
                </Cell>
                {point.requests > 0 ? (
                  <>
                    <Cell><Mono>{rate(point.requests)}</Mono></Cell>
                    <Cell><Badge tone={tone(errors)}>{percent(errors * 100)}</Badge></Cell>
                  </>
                ) : (
                  // Across the requests and their errors
                  <Cell span={2} class="text-base-content/45">
                    last request <Time at={lastSeen} />
                  </Cell>
                )}
                <Cell><Mono>{ms(point.p99)}</Mono></Cell>
                <Cell phone={false} class="w-[220px] py-0">
                  <Sparkline
                    class="block h-7"
                    values={history.map((one) => one.hosts[host]?.requests ?? 0)}
                    color={errors > 0 ? 'var(--color-warning)' : 'var(--color-success)'}
                  />
                </Cell>
              </Row>
            );
          })}
        </Table>
      )}
    </Panel>
  );
}

function Instances({ status }: { status: Status }) {
  const replicas = status.store.replicas;
  if (replicas.length === 0) return null;
  return (
    <Panel
      title={`Sharing the ${status.store.type} store`}
      aside={
        <Legend>
          <span>
            store revision <LegendValue>{status.store.revision ?? '—'}</LegendValue>
          </span>
        </Legend>
      }
    >
      <Table columns={['Hostname', 'Version', 'Applied revision', 'Reported']}>
        {replicas.map((replica) => (
          <Row>
            <Cell>
              {replica.hostname}
              {replica.id === status.replica && <Badge class="ml-2">this one</Badge>}
            </Cell>
            <Cell><Mono>{replica.version}</Mono></Cell>
            <Cell>
              <Badge tone={replica.revision === status.store.revision ? 'success' : 'warning'}>{replica.revision}</Badge>
            </Cell>
            <Cell class="text-base-content/45">
              <Time at={replica.applied_at * 1000} />
            </Cell>
          </Row>
        ))}
      </Table>
    </Panel>
  );
}

export function Overview() {
  const [span, setSpan] = useState(300);
  const { history, latest } = useLive();
  const [status] = useData<Status>('/status', 10000);
  const [tcp] = useData<TcpBlock[]>('/tcp', 10000);
  const last = history[history.length - 1];
  const process = latest?.process;
  const memoryLimit = process?.memory_limit_bytes ?? null;

  return (
    <>
      <PageHead
        title="Overview"
        description="Live, from the moment this page was opened. Nothing more is kept by Proksi."
        actions={
          <Segmented
            label="Time span"
            options={SPANS.map((option) => ({ value: String(option.points), label: option.label }))}
            value={String(span)}
            onChange={(points) => setSpan(Number(points))}
          />
        }
      />

      <Cards columns={5}>
        <Card label="Requests" value={last?.requests ?? null} format={rate} values={take(history, (point) => point.requests)} color="var(--color-success)" />
        <Card
          label="Errors (5xx)"
          value={last ? last.errors * 100 : null}
          format={percent}
          values={take(history, (point) => point.errors * 100)}
          color="var(--color-error)"
        />
        <Card label="Latency p99" value={last?.p99 ?? null} format={ms} detail={last ? `p50 ${ms(last.p50)}` : undefined} values={take(history, (point) => point.p99)} color="var(--chart-3)" />
        <Card
          label="CPU"
          value={last?.cpu ?? null}
          format={percent}
          detail={process?.cpu_limit ? `of 1 core · limit ${process.cpu_limit} cores` : 'of 1 core'}
          values={take(history, (point) => point.cpu)}
          color="var(--chart-2)"
        />
        <Card
          label="Memory"
          value={last?.memory ?? null}
          format={bytes}
          detail={memoryLimit ? `of ${bytes(memoryLimit)}` : last ? `allocator ${bytes(last.allocator)}` : undefined}
          values={take(history, (point) => point.memory)}
          color="var(--chart-4)"
        />
      </Cards>

      <Grid columns={2}>
        <LineChart
          title="Requests by status"
          span={span}
          format={rate}
          series={['2xx', '3xx', '4xx', '5xx'].map((status) => ({
            name: status,
            color: COLORS[status as keyof typeof COLORS],
            values: take(history, (point) => point.responses[status] ?? 0),
          }))}
        />
        <LineChart
          title="Latency"
          span={span}
          format={ms}
          series={(['p50', 'p90', 'p99'] as const).map((quantile) => ({
            name: quantile,
            color: COLORS[quantile],
            values: take(history, (point) => point[quantile]),
          }))}
        />
        <LineChart
          title="CPU"
          span={span}
          format={percent}
          max={process?.cpu_limit ? Math.max(100, process.cpu_limit * 100) : undefined}
          series={[{ name: 'used', color: 'var(--chart-2)', values: take(history, (point) => point.cpu) }]}
        />
        <LineChart
          title="Memory"
          span={span}
          format={bytes}
          max={memoryLimit ?? undefined}
          series={[
            { name: 'resident', color: 'var(--chart-4)', values: take(history, (point) => point.memory) },
            { name: 'allocator', color: 'var(--chart-1)', values: take(history, (point) => point.allocator) },
          ]}
        />
      </Grid>

      <PageSection title="Right now">
        <ActiveHosts latest={latest} history={history} />
        {tcp.state === 'ready' && <TcpBlocks blocks={tcp.value} withHost />}
      </PageSection>

      <PageSection title="Latest requests">
        <RequestLog title={null} />
      </PageSection>

      <Loaded data={status}>
        {(status) =>
          status.store.replicas.length > 0 && (
            <PageSection title="Instances">
              <Instances status={status} />
            </PageSection>
          )
        }
      </Loaded>
    </>
  );
}
