import type { Certificate } from '../api';
import { Badge } from '../components/ui/Badge';
import { Empty, Mono, None, PageHead, Panel } from '../components/ui/Page';
import { Loaded } from '../components/ui/State';
import { Stack } from '../components/ui/Stack';
import { Meter } from '../components/ui/Stats';
import { Cell, Row, Table } from '../components/ui/Table';
import { useData } from '../api/useData';

function Expiry({ days }: { days: number | null }) {
  if (days === null) return <None>unknown</None>;
  const tone = days < 7 ? 'error' : days < 21 ? 'warning' : 'success';
  return (
    <Stack gap={2.5} class="min-w-[180px]">
      <Meter tone={tone} value={Math.max(4, Math.min(90, days))} max={90} />
      <Badge tone={tone}>{days < 0 ? 'expired' : `${days} days`}</Badge>
    </Stack>
  );
}

export function Certificates() {
  const [data] = useData<Certificate[]>('/certificates', 30000);
  return (
    <>
      <PageHead title="Certificates" description="Issued and renewed by Proksi, or loaded from files." />
      <Loaded data={data}>
        {(certificates) => (
          <Panel>
            {certificates.length === 0 ? (
              <Empty title="No certificates">Hosts with TLS get one when Let's Encrypt is enabled.</Empty>
            ) : (
              <Table columns={['Domain', 'Issuer', 'Expires', { label: 'Renewing', phone: false }]}>
                {certificates.map((certificate) => (
                  <Row>
                    <Cell class="font-semibold">{certificate.domain}</Cell>
                    <Cell>
                      {certificate.self_signed ? <Badge tone="warning">self-signed</Badge> : certificate.issuer ?? <None>unknown</None>}
                    </Cell>
                    <Cell><Expiry days={certificate.days_left} /></Cell>
                    <Cell phone={false}>{certificate.lease_holder ? <Mono>{certificate.lease_holder}</Mono> : <None>no</None>}</Cell>
                  </Row>
                ))}
              </Table>
            )}
          </Panel>
        )}
      </Loaded>
    </>
  );
}
