import type { ComponentChildren } from 'preact';
import { session as api, type Session, type Status } from '../api';
import { onLink } from '../router';
import { useData } from '../useData';
import { Globe, History, Pulse, Server, Shield } from './icons';

const NAV = [
  { href: '/overview', label: 'Overview', icon: Pulse },
  { href: '/hosts', label: 'Hosts', icon: Globe },
  { href: '/certificates', label: 'Certificates', icon: Shield },
  { href: '/server', label: 'Server', icon: Server },
  { href: '/changes', label: 'Changes', icon: History, soon: true },
];

function uptime(seconds: number) {
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

type Props = { path: string; session: Session; onSignOut: () => void; children: ComponentChildren };

export function Layout({ path, session, onSignOut, children }: Props) {
  const [status] = useData<Status>('/status', 15000);
  const ready = status.state === 'ready' ? status.value : null;
  return (
    <div class="shell">
      <aside class="sidebar">
        <div class="brand">
          <img src="/logo.svg" alt="" />
          Proksi
          {ready && <small>v{ready.version}</small>}
        </div>
        <nav class="nav">
          {NAV.map(({ href, label, icon: Icon, soon }) =>
            soon ? (
              <a key={href} aria-disabled="true" title="Coming soon">
                <Icon />
                {label}
                <span class="soon">soon</span>
              </a>
            ) : (
              <a key={href} href={href} onClick={onLink} aria-current={path.startsWith(href) ? 'page' : undefined}>
                <Icon />
                {label}
              </a>
            ),
          )}
        </nav>
        {ready && (
          <footer>
            <div class="row">
              <span class="dot ok" /> {ready.hostname}
            </div>
            <div>
              up {uptime(ready.uptime_seconds)} · {ready.store.type} store
            </div>
            {session.name && (
              <div class="row session">
                <span>
                  {session.name}
                  {session.access === 'read' && <span class="badge">read-only</span>}
                </span>
                <button class="link" onClick={() => api.signOut().finally(onSignOut)}>
                  Sign out
                </button>
              </div>
            )}
          </footer>
        )}
      </aside>
      <main>{children}</main>
    </div>
  );
}
