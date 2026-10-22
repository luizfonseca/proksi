import type { ComponentChildren } from 'preact';
import { useState } from 'preact/hooks';
import { session as api, type Session, type Status as ServerStatus } from '../api';
import { onLink } from '../router';
import { useData } from '../api/useData';
import { Badge, Status } from './ui/Badge';
import { Button } from './ui/Button';
import { Bell, Book, External, Globe, Pulse, Server, Shield } from './ui/icons';
import { Palette, PaletteButton, SHORTCUT } from '../features/palette/Palette';
import { Toaster } from './ui/Toast';
import { DOCS, GITHUB, ISSUES } from '../lib/links';

const NAV = [
  { href: '/overview', label: 'Overview', icon: Pulse },
  { href: '/hosts', label: 'Hosts', icon: Globe },
  { href: '/certificates', label: 'Certificates', icon: Shield },
  { href: '/notifications', label: 'Notifications', icon: Bell },
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

/** Who's signed in: their initial, opening their name, what they can do, and signing out */
function Account({ session, onSignOut }: { session: Session; onSignOut: () => void }) {
  const [open, setOpen] = useState(false);
  if (!session.name) return null;
  return (
    <div
      class="relative"
      onKeyDown={(e) => e.key === 'Escape' && setOpen(false)}
      onFocusOut={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as globalThis.Node | null)) setOpen(false);
      }}
    >
      <button
        type="button"
        class="avatar avatar-placeholder cursor-pointer"
        title={session.name}
        aria-label={`Signed in as ${session.name}`}
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <div class="w-8 rounded-full border border-base-300 bg-base-200 text-sm font-semibold text-base-content/80 hover:bg-base-300">
          <span>{session.name.slice(0, 1).toUpperCase()}</span>
        </div>
      </button>
      {open && (
        <div class="absolute top-[calc(100%+8px)] right-0 z-20 w-56 rounded-box border border-base-300 bg-base-100 p-3 shadow-lg">
          <div class="text-xs text-base-content/60">Signed in as</div>
          <div class="mt-0.5 flex items-center gap-2 font-semibold">
            <span class="truncate">{session.name}</span>
            <Badge size="xs" tone={session.access === 'write' ? 'primary' : 'neutral'}>
              {session.access === 'write' ? 'read & write' : 'read-only'}
            </Badge>
          </div>
          <div class="mt-3 grid gap-1.5 border-t border-base-300 pt-2.5 text-sm">
            <a class="flex items-center gap-2 text-base-content/70 hover:text-base-content" href={DOCS} target="_blank" rel="noreferrer">
              <Book /> Documentation
            </a>
            <a class="flex items-center gap-2 text-base-content/70 hover:text-base-content" href={GITHUB} target="_blank" rel="noreferrer">
              <External /> Proksi on GitHub
            </a>
          </div>
          <div class="mt-2.5 border-t border-base-300 pt-2">
            <Button variant="link" onClick={onSignOut}>
              Sign out
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

/** Under every page: the version, and where to read about Proksi or ask about it */
function Footer({ version }: { version?: string }) {
  const link = 'hover:text-base-content';
  return (
    <footer class="border-t border-base-300">
      <div class={`${WIDTH} flex flex-wrap items-center gap-x-5 gap-y-1 py-5 text-xs text-base-content/45`}>
        <span>Proksi{version && ` v${version}`}</span>
        <a class={link} href={DOCS} target="_blank" rel="noreferrer">
          Documentation
        </a>
        <a class={link} href={GITHUB} target="_blank" rel="noreferrer">
          GitHub
        </a>
        <a class={link} href={ISSUES} target="_blank" rel="noreferrer">
          Report an issue
        </a>
        <span class="ml-auto max-md:hidden">
          Press <kbd class="kbd kbd-xs border-base-300 bg-transparent shadow-none">{SHORTCUT}</kbd> to search
        </span>
      </div>
    </footer>
  );
}

/** The UI's pages, the current one underlined: stays at the top as the page scrolls */
function Nav({ path }: { path: string }) {
  return (
    <nav class="sticky top-0 z-10 border-b border-base-300 bg-base-100">
      {/* Sideways on phones, without a scrollbar */}
      <ul class={`${WIDTH} flex gap-1 overflow-x-auto overflow-y-hidden [scrollbar-width:none]`}>
        {NAV.map(({ href, label, icon: Icon }) => {
          const current = path.startsWith(href);
          return (
            <li key={href}>
              <a
                class={`flex items-center gap-2 border-b-2 px-3 py-3 text-sm font-medium whitespace-nowrap ${
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
    <div class="flex min-h-screen flex-col">
      <Palette onSignOut={signOut} />
      <Toaster />
      <header class="border-b border-base-300 bg-base-100">
        <div class={`${WIDTH} flex h-14 items-center gap-3`}>
          <Brand version={ready?.version} />
          <div class="ml-auto flex items-center gap-2">
            {ready && <Instance status={ready} />}
            <PaletteButton />
            <Account session={session} onSignOut={signOut} />
          </div>
        </div>
      </header>
      <Nav path={path} />
      <main class={`${WIDTH} grow pt-6 pb-12`}>{children}</main>
      <Footer version={ready?.version} />
    </div>
  );
}
