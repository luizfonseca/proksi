import type { ComponentChildren } from 'preact';
import { session as api, type Session, type Status as ServerStatus } from '../api';
import { onLink } from '../router';
import { useData } from '../api/useData';
import { Badge, Status } from './ui/Badge';
import { Button } from './ui/Button';
import { Globe, Pulse, Server, Shield } from './ui/icons';

const NAV = [
  { href: '/overview', label: 'Overview', icon: Pulse },
  { href: '/hosts', label: 'Hosts', icon: Globe },
  { href: '/certificates', label: 'Certificates', icon: Shield },
  { href: '/server', label: 'Server', icon: Server },
];

function uptime(seconds: number) {
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

/** The logo, and the version once known */
function Brand({ version }: { version?: string }) {
  return (
    <div class="flex shrink-0 items-center gap-2.5 px-2.5 text-base font-semibold md:pt-1 md:pb-4">
      <img class="w-7" src="/logo.svg" alt="" />
      Proksi
      {version && <small class="ml-auto hidden font-mono text-xs font-medium text-base-content/45 md:inline">v{version}</small>}
    </div>
  );
}

/** The UI's pages: a column on the side, a row on phones */
function Nav({ path }: { path: string }) {
  return (
    <ul class="menu menu-horizontal flex-nowrap p-0 [--menu-active-bg:var(--color-base-200)] [--menu-active-fg:var(--color-base-content)] md:menu-vertical md:w-full">
      {NAV.map(({ href, label, icon: Icon }) => {
        const current = path.startsWith(href);
        return (
          <li key={href}>
            <a class={`font-medium ${current ? 'menu-active' : 'text-base-content/60'}`} href={href} onClick={onLink} aria-current={current ? 'page' : undefined}>
              <Icon />
              {label}
            </a>
          </li>
        );
      })}
    </ul>
  );
}

/** Which instance this is, and who's signed in */
function Footer({ status, session, onSignOut }: { status: ServerStatus; session: Session; onSignOut: () => void }) {
  return (
    <footer class="ml-auto p-2.5 text-xs text-base-content/45 md:mt-auto md:ml-0">
      <div class="hidden items-center gap-1.5 md:flex">
        <Status tone="success" /> {status.hostname}
      </div>
      <div class="hidden md:block">
        up {uptime(status.uptime_seconds)} · {status.store.type} store
      </div>
      {session.name && (
        <div class="flex items-center justify-between gap-3 text-base-content/60 md:mt-2 md:border-t md:border-base-300 md:pt-2">
          <span class="whitespace-nowrap">
            {session.name}
            {session.access === 'read' && <Badge size="xs" class="ml-1.5">read-only</Badge>}
          </span>
          <Button variant="link" onClick={() => api.signOut().finally(onSignOut)}>
            Sign out
          </Button>
        </div>
      )}
    </footer>
  );
}

type Props = { path: string; session: Session; onSignOut: () => void; children: ComponentChildren };

/** The sidebar and the page */
export function Layout({ path, session, onSignOut, children }: Props) {
  const [status] = useData<ServerStatus>('/status', 15000);
  const ready = status.state === 'ready' ? status.value : null;
  return (
    <div class="min-h-screen md:grid md:grid-cols-[232px_minmax(0,1fr)]">
      <aside class="flex items-center gap-1 overflow-x-auto border-b border-base-300 bg-base-100 px-3 py-2.5 md:sticky md:top-0 md:h-screen md:flex-col md:items-stretch md:border-r md:border-b-0 md:px-3 md:py-4">
        <Brand version={ready?.version} />
        <Nav path={path} />
        {ready && <Footer status={ready} session={session} onSignOut={onSignOut} />}
      </aside>
      <main class="max-w-[1180px] min-w-0 px-4 pt-5 pb-12 md:px-9 md:pt-7 md:pb-16">{children}</main>
    </div>
  );
}
