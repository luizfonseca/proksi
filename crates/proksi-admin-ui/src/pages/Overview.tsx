import { useState } from 'preact/hooks';
import type { Status } from '../api';
import { LineChart, Sparkline } from '../components/Chart';
import { RequestLog } from '../components/RequestLog';
import { Loaded } from '../components/State';
import { bytes, ms, percent, rate } from '../format';
import { type Point, useLive } from '../live';
import { useData } from '../useData';

const SPANS = [
  { label: '1m', points: 60 },
  { label: '5m', points: 300 },
  { label: '15m', points: 900 },
];

const COLORS = {
  '2xx': 'var(--ok)',
  '3xx': 'var(--info-text)',
  '4xx': 'var(--warn)',
  '5xx': 'var(--bad)',
  p50: 'var(--chart-1)',
  p90: 'var(--chart-2)',
  p99: 'var(--chart-3)',
};

function Card({ label, value, detail, values, color }: { label: string; value: string; detail?: string; values: (number | null)[]; color: string }) {
  return (
    <div class="panel stat">
      <div class="label">{label}</div>
      <div class="value">{value}</div>
      {detail && <div class="detail">{detail}</div>}
      <Sparkline values={values} color={color} />
    </div>
  );
}

const take = (history: Point[], read: (point: Point) => number | null) => history.map(read);

function Instances({ status }: { status: Status }) {
  const replicas = status.store.replicas;
  if (replicas.length === 0) return null;
  return (
    <section class="panel">
      <div class="chart-head">
        <h3>Instances sharing the {status.store.type} store</h3>
        <div class="legend">
          <span>store revision <b>{status.store.revision ?? '—'}</b></span>
        </div>
      </div>
      <table>
        <thead>
          <tr>
            <th>Hostname</th>
            <th>Version</th>
            <th>Applied revision</th>
            <th>Reported</th>
          </tr>
        </thead>
        <tbody>
          {replicas.map((replica) => (
            <tr>
              <td>
                {replica.hostname}
                {replica.id === status.replica && <span class="badge" style={{ marginLeft: 8 }}>this one</span>}
              </td>
              <td class="mono">{replica.version}</td>
              <td>
                <span class={`badge ${replica.revision === status.store.revision ? 'ok' : 'warn'}`}>{replica.revision}</span>
              </td>
              <td class="muted">{new Date(replica.applied_at * 1000).toLocaleTimeString()}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

export function Overview() {
  const [span, setSpan] = useState(300);
  const { history, latest } = useLive();
  const [status] = useData<Status>('/status', 10000);
  const last = history[history.length - 1];
  const process = latest?.process;
  const memoryLimit = process?.memory_limit_bytes ?? null;

  return (
    <>
      <div class="page-head">
        <div>
          <h1>Overview</h1>
          <p>Live, from the moment this page was opened. Nothing more is kept by Proksi.</p>
        </div>
        <div class="actions">
          <div class="segmented" role="group" aria-label="Time span">
            {SPANS.map((option) => (
              <button type="button" aria-pressed={span === option.points} onClick={() => setSpan(option.points)}>
                {option.label}
              </button>
            ))}
          </div>
        </div>
      </div>

      <div class="stats overview-cards">
        <Card label="Requests" value={last ? rate(last.requests) : '—'} values={take(history, (point) => point.requests)} color="var(--chart-1)" />
        <Card
          label="Errors (5xx)"
          value={last ? percent(last.errors * 100) : '—'}
          values={take(history, (point) => point.errors * 100)}
          color="var(--bad)"
        />
        <Card label="Latency p99" value={last ? ms(last.p99) : '—'} detail={last ? `p50 ${ms(last.p50)}` : undefined} values={take(history, (point) => point.p99)} color="var(--chart-3)" />
        <Card
          label="CPU"
          value={last ? percent(last.cpu) : '—'}
          detail={process?.cpu_limit ? `of 1 core · limit ${process.cpu_limit} cores` : 'of 1 core'}
          values={take(history, (point) => point.cpu)}
          color="var(--chart-2)"
        />
        <Card
          label="Memory"
          value={last ? bytes(last.memory) : '—'}
          detail={memoryLimit ? `of ${bytes(memoryLimit)}` : last ? `allocator ${bytes(last.allocator)}` : undefined}
          values={take(history, (point) => point.memory)}
          color="var(--chart-4)"
        />
      </div>

      <div class="charts">
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
      </div>

      <RequestLog />
      <Loaded data={status}>{(status) => <Instances status={status} />}</Loaded>
    </>
  );
}
