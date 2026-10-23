import { useState } from 'preact/hooks';
import { type Delivery, type Host, type Notification, notifications as api, type Notifications as Data } from '../api';
import { useCanWrite } from '../api/session';
import { useData } from '../api/useData';
import { Badge, Chip, SourceBadge } from '../components/ui/Badge';
import { Button } from '../components/ui/Button';
import { Empty, Hint, Mono, None, PageHead, Panel, Section } from '../components/ui/Page';
import { Stack } from '../components/ui/Stack';
import { Loaded } from '../components/ui/State';
import { Time } from '../components/ui/Time';
import { toast } from '../components/ui/Toast';
import { covered } from '../features/notify/notify';
import { DOCS } from '../lib/links';

function Last({ delivery }: { delivery: Delivery | null }) {
  if (!delivery) return <None>nothing sent yet</None>;
  return (
    <div class="grid gap-1">
      <Stack gap={1.5} wrap>
        <Badge tone={delivery.ok ? 'success' : 'error'}>{delivery.ok ? 'sent' : 'failed'}</Badge>
        <Mono>{delivery.event}</Mono>
        <Time at={delivery.at * 1000} exact class="text-xs text-base-content/60" />
      </Stack>
      {delivery.error && <Hint block>{delivery.error}</Hint>}
    </div>
  );
}

/** The hosts whose events the block is told about, each opening its page */
function Hosts({ block, hosts }: { block: Notification; hosts: string[] | null }) {
  if (hosts === null) return <None>{block.hosts.length === 0 ? 'every host' : block.hosts.join(', ')}</None>;
  const found = covered(block, hosts);
  if (found.length === 0) return <None>no host matches yet</None>;
  return (
    <Stack gap={1} wrap>
      {found.map((host) => <Chip href={`/hosts/${encodeURIComponent(host)}`}>{host}</Chip>)}
    </Stack>
  );
}

function Block({ block, hosts, onTested }: { block: Notification; hosts: string[] | null; onTested: () => void }) {
  const canWrite = useCanWrite();
  const [testing, setTesting] = useState(false);

  async function remove() {
    if (!confirm(`Remove ${block.name}? Every instance stops sending it events.`)) return;
    try {
      await api.delete(block.name, block.version === null ? undefined : { version: block.version });
      toast({ message: `${block.name} removed` });
    } catch (err) {
      toast({ message: `${block.name} wasn't removed: ${(err as Error).message ?? err}`, tone: 'info' });
    } finally {
      onTested();
    }
  }

  async function test() {
    setTesting(true);
    try {
      await api.test(block.name);
      toast({ message: `A test was sent to ${block.name}` });
    } catch {
      toast({ message: `The test to ${block.name} failed`, tone: 'info' });
    } finally {
      setTesting(false);
      onTested();
    }
  }

  return (
    <Panel>
      <Stack gap={3} wrap class="border-b border-base-300 px-4 py-3">
        <span class="font-semibold">{block.name}</span>
        <SourceBadge source={block.source} />
        <Badge tone="info">{block.format}</Badge>
        <Mono class="text-base-content/60">{block.url}</Mono>
        {canWrite && (
          <Stack gap={2} class="ml-auto">
            {block.source === 'store' && (
              <>
                <Button variant="link-danger" onClick={remove}>Remove</Button>
                <Button href={`/notifications/${encodeURIComponent(block.name)}/edit`}>Edit</Button>
              </>
            )}
            <Button class="whitespace-nowrap" disabled={testing} onClick={test}>
              {testing ? 'Sending…' : 'Send a test'}
            </Button>
          </Stack>
        )}
      </Stack>
      <Section label="Events">
        {block.on.length === 0 ? (
          <None>every event</None>
        ) : (
          <Stack gap={1} wrap>
            {block.on.map((event) => <Chip>{event}</Chip>)}
          </Stack>
        )}
      </Section>
      <Section label="Hosts" hint={block.hosts.length === 0 ? 'every host' : block.hosts.join(', ')}>
        <Hosts block={block} hosts={hosts} />
      </Section>
      {block.headers.length > 0 && (
        <Section label="Headers" hint="values hidden">
          <Stack gap={1} wrap>
            {block.headers.map((header) => <Chip>{header}</Chip>)}
          </Stack>
        </Section>
      )}
      <Section label="Last send">
        <Last delivery={block.last} />
      </Section>
    </Panel>
  );
}

export function Notifications() {
  const [data, reload] = useData<Data>('/notifications', 15000);
  const canWrite = useCanWrite();
  const [served] = useData<Host[]>('/hosts', 30000);
  const hosts = served.state === 'ready' ? served.value.map((host) => host.host) : null;
  return (
    <>
      <PageHead
        title="Notifications"
        description="Webhooks told when upstreams go down, certificates fail or expire, or the configuration or the store change."
        actions={
          canWrite && (
            <Button variant="primary" href="/notifications/new">
              New notification
            </Button>
          )
        }
      />
      <Loaded data={data}>
        {({ notifications }) =>
          notifications.length === 0 ? (
            <Panel>
              <Empty title="Nothing configured">
                Add a <Mono>notify</Mono> block to the configuration{canWrite && ', or create one here,'} to send events to a webhook.{' '}
                <Button variant="link" href={`${DOCS}/configuration/notify`} external>
                  How
                </Button>
              </Empty>
            </Panel>
          ) : (
            <div class="grid grid-cols-1 gap-3">
              {notifications.map((block) => <Block block={block} hosts={hosts} onTested={reload} />)}
            </div>
          )
        }
      </Loaded>
    </>
  );
}
