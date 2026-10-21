import type { Certificate } from '../api';
import { Loaded } from '../components/ui/State';
import { useData } from '../api/useData';

function Expiry({ days }: { days: number | null }) {
  if (days === null) return <span class="none">unknown</span>;
  const tone = days < 7 ? 'bad' : days < 21 ? 'warn' : 'ok';
  const width = Math.max(4, Math.min(100, (days / 90) * 100));
  return (
    <div class="expiry">
      <div class="bar">
        <span style={{ width: `${width}%`, background: `var(--${tone})` }} />
      </div>
      <span class={`badge ${tone}`}>{days < 0 ? 'expired' : `${days} days`}</span>
    </div>
  );
}

export function Certificates() {
  const [data] = useData<Certificate[]>('/certificates', 30000);
  return (
    <>
      <div class="page-head">
        <div>
          <h1>Certificates</h1>
          <p>Issued and renewed by Proksi, or loaded from files.</p>
        </div>
      </div>
      <Loaded data={data}>
        {(certificates) => (
          <div class="panel">
            {certificates.length === 0 ? (
              <div class="empty">
                <strong>No certificates</strong>
                Hosts with TLS get one when Let's Encrypt is enabled.
              </div>
            ) : (
              <table>
                <thead>
                  <tr>
                    <th>Domain</th>
                    <th>Issuer</th>
                    <th>Expires</th>
                    <th class="hide-sm">Renewing</th>
                  </tr>
                </thead>
                <tbody>
                  {certificates.map((certificate) => (
                    <tr>
                      <td class="host-name">{certificate.domain}</td>
                      <td>
                        {certificate.self_signed ? <span class="badge warn">self-signed</span> : certificate.issuer ?? <span class="none">unknown</span>}
                      </td>
                      <td><Expiry days={certificate.days_left} /></td>
                      <td class="hide-sm">{certificate.lease_holder ? <span class="mono">{certificate.lease_holder}</span> : <span class="none">no</span>}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        )}
      </Loaded>
    </>
  );
}
