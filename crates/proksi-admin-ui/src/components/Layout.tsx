import type { ComponentChildren } from 'preact';
import { session as api, type Session, type Status as ServerStatus } from '../api';
import { onLink } from '../router';
import { useData } from '../api/useData';
import { Badge, Status } from './ui/Badge';
import { Button } from './ui/Button';
import { Globe, Pulse, Server, Shield } from './ui/icons';
import { ThemeToggle } from './ui/Theme';
import { Palette, PaletteButton } from '../features/palette/Palette';

const NAV = [
  { href: '/overview', label: 'Overview', icon: Pulse },
  { href: '/hosts', label: 'Hosts', icon: Globe },
  { href: '/certificates', label: 'Certificates', icon: Shield },
  { href: '/server', label: 'Server', icon: Server },
];

/** The width of the bars and the page, centered */
const WIDTH = 'mx-auto w-full max-w-7xl px-4 md:px-6';

function uptime(seconds: number) {
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

/** The logo, and the version once known */
function Brand({ version }: { version?: string }) {
  return (
    <a class="flex shrink-0 items-center gap-2.5 text-lg font-semibold" href="/overview" onClick={onLink}>
      <img class="w-7" src="/logo.svg" alt="" />
      Proksi
      {version && <small class="font-mono text-xs font-medium text-base-content/45">v{version}</small>}
    </a>
  );
}

/** Which instance this is */
function Instance({ status }: { status: ServerStatus }) {
  return (
    <span class="hidden items-center gap-1.5 text-xs text-base-content/60 lg:flex" title={`${status.store.type} store`}>
      <Status tone="success" /> {status.hostname} · up {uptime(status.uptime_seconds)}
    </span>
  );
}

/** Who's signed in */
function Account({ session, onSignOut }: { session: Session; onSignOut: () => void }) {
  if (!session.name) return null;
  return (
    <span class="flex items-center gap-3 border-l border-base-300 pl-3 text-sm">
      <span class="whitespace-nowrap">
        {session.name}
        {session.access === 'read' && <Badge size="xs" class="ml-1.5">read-only</Badge>}
      </span>
      <Button variant="link" onClick={onSignOut}>
        Sign out
      </Button>
    </span>
  );
}

/** The UI's pages, the current one underlined */
function Nav({ path }: { path: string }) {
  return (
    <nav class="border-t border-base-300">
      <ul class={`${WIDTH} flex gap-1 overflow-x-auto`}>
        {NAV.map(({ href, label, icon: Icon }) => {
          const current = path.startsWith(href);
          return (
            <li key={href}>
              <a
                class={`-mb-px flex items-center gap-2 border-b-2 px-3 py-3 text-sm font-medium whitespace-nowrap ${
                  current ? 'border-primary text-base-content' : 'border-transparent text-base-content/60 hover:text-base-content'
                }`}
                href={href}
                onClick={onLink}
                aria-current={current ? 'page' : undefined}
              >
                <Icon />
                {label}
              </a>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

type Props = { path: string; session: Session; onSignOut: () => void; children: ComponentChildren };

/** The bars at the top, and the page */
export function Layout({ path, session, onSignOut, children }: Props) {
  const [status] = useData<ServerStatus>('/status', 15000);
  const ready = status.state === 'ready' ? status.value : null;
  const signOut = () => api.signOut().finally(onSignOut);
  return (
    <div class="min-h-screen">
      <Palette onSignOut={signOut} />
      <header class="border-b border-base-300 bg-base-100">
        <div class={`${WIDTH} flex h-14 items-center gap-3`}>
          <Brand version={ready?.version} />
          <PaletteButton />
          <div class="ml-auto flex items-center gap-3">
            {ready && <Instance status={ready} />}
            <ThemeToggle />
            <Account session={session} onSignOut={signOut} />
          </div>
        </div>
        <Nav path={path} />
      </header>
      <main class={`${WIDTH} pt-6 pb-16`}>{children}</main>
    </div>
  );
}
