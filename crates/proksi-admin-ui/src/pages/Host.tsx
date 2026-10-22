import { useState } from 'preact/hooks';
import { type Host as HostData, routes, type TcpBlock, type Upstream } from '../api';
import { Legend, LineChart, Swatch } from '../components/ui/Chart';
import { Flow } from '../features/flow/Flow';
import { RequestLog } from '../features/live/RequestLog';
import { Rules } from '../features/rules/Rules';
import { TcpBlocks } from '../features/tcp/Tcp';
import { layout } from '../features/flow/flowGraph';
import { ms, rate } from '../lib/format';
import { useLive } from '../features/live/live';
import { Badge, Chip, ProtocolBadge, SourceBadge, Status, UpstreamHealth } from '../components/ui/Badge';
import { Button } from '../components/ui/Button';
import { Empty, Hint, Mono, None, PageHead, Panel, Section } from '../components/ui/Page';
import { Grid, Stack } from '../components/ui/Stack';
import { Loaded } from '../components/ui/State';
import { navigate, onLink } from '../router';
import { useCanWrite } from '../api/session';
import { useData } from '../api/useData';

function UpstreamRow({ upstream }: { upstream: Upstream }) {
  // Addresses worth showing: a name's, or a service's
  const resolved = upstream.addresses.filter((address) => address.address !== upstream.upstream);
  const tagged = Boolean(upstream.split || upstream.mirror);
  return (
    <Stack gap={2.5} class="py-1">
      <Status tone={upstream.healthy ? 'success' : 'error'} />
      <div class="min-w-0">
        <Mono class="block">
          {upstream.name ? <span class="text-base-content/60">{upstream.name} → </span> : null}
          {upstream.upstream}
        </Mono>
        {resolved.length > 0 && !upstream.name && (
          <Hint class="block font-mono">
            {resolved.map((address) => (
              <span title={address.healthy ? 'healthy' : 'unhealthy'}>{address.address} </span>
            ))}
          </Hint>
        )}
        {upstream.addresses.length === 0 && <Hint class="block font-mono">not resolved</Hint>}
      </div>
      {upstream.split && (
        <Badge tone="secondary" class="ml-auto" title="Traffic split: its share of clients">
          {upstream.split.name} · {upstream.split.percent}%
        </Badge>
      )}
      {upstream.mirror && (
        <Badge tone="accent" class={upstream.split ? '' : 'ml-auto'} title="Mirror: gets copies of the requests, its answers are dropped">
          mirror
        </Badge>
      )}
      <Hint class={`${tagged ? 'ml-2' : 'ml-auto'} whitespace-nowrap`}>weight {upstream.weight}</Hint>
    </Stack>
  );
}

function Actions({ name }: { name: string }) {
  const [error, setError] = useState<string | null>(null);
  async function remove() {
    if (!confirm(`Remove ${name} from the store? Every instance stops serving it. Its versions are kept, to restore it.`)) return;
    try {
      await routes.delete(name);
      navigate('/hosts');
    } catch (err) {
      setError(String((err as Error).message ?? err));
    }
  }
  return (
    <>
      {error && <span class="self-center text-error">{error}</span>}
      <Button variant="danger" onClick={remove}>Remove</Button>
      <Button href={`/hosts/${encodeURIComponent(name)}/history`}>History</Button>
      <Button href={`/hosts/${encodeURIComponent(name)}/edit`}>Edit</Button>
    </>
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
      <Panel
        class="mb-3"
        title="Traffic"
        aside={
          <Legend>
            <Swatch color="var(--color-success)">flowing</Swatch>
            <Swatch color="var(--color-warning)">some errors</Swatch>
            <Swatch color="var(--color-error)">failing or unhealthy</Swatch>
            <span>over the last 5 seconds</span>
          </Legend>
        }
      >
        <div class="px-4 pb-2.5">
          <Flow flow={layout(host, stats, latest?.window_seconds)} />
        </div>
      </Panel>
      <Grid columns={2} class="mb-3">
        <LineChart
          title="Requests"
          span={300}
          format={rate}
          series={[
            { name: 'all', color: 'var(--color-success)', values: read((point) => point.requests) },
            { name: '5xx', color: 'var(--color-error)', values: read((point) => point.errors) },
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
      </Grid>
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
            <Panel>
              <Empty title={`No host named ${name}`}>
                <Button variant="link" href="/hosts">Back to hosts</Button>
              </Empty>
            </Panel>
          );
        }
        return (
          <>
            <PageHead
              crumbs={
                <ul>
                  <li><a href="/hosts" onClick={onLink}>Hosts</a></li>
                  <li>{host.host}</li>
                </ul>
              }
              title={host.host}
              badges={
                <>
                  <SourceBadge source={host.source} />
                  {host.locations.length > 0 && <ProtocolBadge protocol="http" tls={host.tls} />}
                  {blocks.length > 0 && <ProtocolBadge protocol="tcp" />}
                  {host.listen.map((address) => <Chip>{address}</Chip>)}
                </>
              }
              actions={canWrite && host.source === 'store' && <Actions name={host.host} />}
            />
            <Traffic host={host} />
            {host.rules.length > 0 && (
              <Panel class="mb-3">
                <Section label="Host rules" hint="every request, before its location is chosen">
                  <Rules rules={host.rules} />
                </Section>
              </Panel>
            )}
            <h2 class="mt-5 mb-2.5 flex items-center gap-2 text-sm font-semibold">
              Locations
              <ProtocolBadge protocol="http" />
            </h2>
            <Grid>
              {host.locations.map((location) => (
                <Panel class="location" id={encodeURIComponent(`location ${location.location}`)}>
                  <Stack gap={3} class="border-b border-base-300 px-4 py-3">
                    <Mono size="md" class="font-semibold">{location.location}</Mono>
                    {location.mirror && <Badge tone="accent">mirrors {location.mirror.percent}%</Badge>}
                    <span class="ml-auto"><UpstreamHealth upstreams={location.upstreams} /></span>
                  </Stack>
                  <Section label="Upstreams">
                    {location.upstreams.length === 0 ? (
                      <None>Answered by Proksi (no upstream)</None>
                    ) : (
                      location.upstreams.map((upstream) => <UpstreamRow upstream={upstream} />)
                    )}
                    {location.mirror && (
                      <Hint block class="mt-1.5" title="Copies of the requests sent to the mirror since Proksi started">
                        Mirror: {location.mirror.sent} sent · {location.mirror.failed} failed
                        {location.mirror.dropped > 0 && ` · ${location.mirror.dropped} dropped (too many at once)`}
                        {location.mirror.too_large > 0 && ` · ${location.mirror.too_large} not copied (body too large)`}
                      </Hint>
                    )}
                  </Section>
                  {location.blocks.length > 0 && (
                    <Section label="Plugins" hint="in the order they run">
                      <Stack gap={1} wrap>
                        {location.blocks.map((block) => <Chip>{block}</Chip>)}
                      </Stack>
                    </Section>
                  )}
                  {location.rules.length > 0 && (
                    <Section label="Rules" hint="each that matches, in order: one that answers ends the request">
                      <Rules rules={location.rules} />
                    </Section>
                  )}
                </Panel>
              ))}
            </Grid>
            <TcpBlocks blocks={blocks} />
          </>
        );
      }}
    </Loaded>
  );
}
