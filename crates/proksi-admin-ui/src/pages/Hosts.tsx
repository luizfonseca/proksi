import { useState } from 'preact/hooks';
import type { Host, TcpBlock } from '../api';
import { Chip, ProtocolBadge, SourceBadge, Status } from '../components/ui/Badge';
import { Button } from '../components/ui/Button';
import { SearchInput, Segmented } from '../components/ui/Form';
import { Empty, Hint, None, PageHead, Panel } from '../components/ui/Page';
import { Stack } from '../components/ui/Stack';
import { Stat, Stats } from '../components/ui/Stats';
import { Loaded } from '../components/ui/State';
import { Cell, Row, Table } from '../components/ui/Table';
import { navigate } from '../router';
import { useCanWrite } from '../api/session';
import { useData } from '../api/useData';

function Health({ host }: { host: Host }) {
  const upstreams = host.locations.flatMap((location) => location.upstreams);
  if (upstreams.length === 0) return <None>no upstreams</None>;
  const healthy = upstreams.filter((upstream) => upstream.healthy).length;
  return (
    <Stack class="text-sm text-base-content/60">
      <Stack gap={1}>
        {upstreams.slice(0, 8).map((upstream) => (
          <Status tone={upstream.healthy ? 'success' : 'error'} title={upstream.upstream} />
        ))}
      </Stack>
      {healthy}/{upstreams.length}
    </Stack>
  );
}

function Summary({ hosts }: { hosts: Host[] }) {
  const upstreams = hosts.flatMap((host) => host.locations.flatMap((location) => location.upstreams));
  const unhealthy = upstreams.filter((upstream) => !upstream.healthy).length;
  const locations = hosts.reduce((count, host) => count + host.locations.length, 0);
  return (
    <Stats columns={4}>
      <Stat title="Hosts" value={hosts.length} />
      <Stat title="Locations" value={locations} />
      <Stat title="Upstreams" value={upstreams.length} />
      <Stat title="Unhealthy upstreams" value={unhealthy} tone={unhealthy ? 'error' : 'none'} />
    </Stats>
  );
}

/** Hosts removed from the store, which their history restores */
function DeletedHosts() {
  const [data] = useData<{ hosts: string[] }>('/deleted-routes', 30000);
  if (data.state !== 'ready' || data.value.hosts.length === 0) return null;
  return (
    <Panel class="mt-4 pb-3.5" title={<>Deleted hosts <Hint class="ml-1.5">their versions are kept: open one to restore it</Hint></>}>
      <Stack gap={1} wrap class="px-4">
        {data.value.hosts.map((host) => (
          <Chip href={`/hosts/${encodeURIComponent(host)}/history`}>{host}</Chip>
        ))}
      </Stack>
    </Panel>
  );
}

/** What a host serves: HTTP requests, TCP connections, or both */
function Serves({ host, tcp }: { host: Host; tcp: boolean }) {
  return (
    <Stack gap={1}>
      {host.locations.length > 0 && <ProtocolBadge protocol="http" tls={host.tls} />}
      {tcp && <ProtocolBadge protocol="tcp" />}
    </Stack>
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
      <PageHead
        title="Hosts"
        description="Every host Proksi serves, from every source."
        actions={canWrite && <Button variant="primary" href="/hosts/new">New host</Button>}
      />
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
              <Stack class="mb-3">
                <SearchInput class="max-w-xs flex-1" placeholder="Search hosts" value={query} onInput={setQuery} />
                {sources.length > 2 && <Segmented label="Source" options={sources} value={source} onChange={setSource} />}
              </Stack>
              <Panel>
                {shown.length === 0 ? (
                  <Empty title={hosts.length === 0 ? 'No hosts yet' : 'No hosts match'}>
                    {hosts.length === 0 ? 'Add a host block to the configuration file.' : 'Try another search.'}
                  </Empty>
                ) : (
                  <Table columns={['Host', 'Serves', 'Source', { label: 'Locations', phone: false }, 'Upstreams', { label: 'Plugins', phone: false }]}>
                    {shown.map((host) => {
                      const blocks = [...new Set(host.locations.flatMap((location) => location.blocks))];
                      return (
                        <Row onClick={() => navigate(`/hosts/${encodeURIComponent(host.host)}`)}>
                          <Cell class="font-semibold">{host.host}</Cell>
                          <Cell><Serves host={host} tcp={tcpHosts.has(host.host)} /></Cell>
                          <Cell><SourceBadge source={host.source} /></Cell>
                          <Cell phone={false} numeric>{host.locations.length}</Cell>
                          <Cell><Health host={host} /></Cell>
                          <Cell phone={false}>
                            <Stack gap={1} wrap>
                              {blocks.length === 0 ? <None>none</None> : blocks.map((block) => <Chip>{block}</Chip>)}
                            </Stack>
                          </Cell>
                        </Row>
                      );
                    })}
                  </Table>
                )}
              </Panel>
              <DeletedHosts />
            </>
          );
        }}
      </Loaded>
    </>
  );
}
