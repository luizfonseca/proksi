import type { ComponentChildren } from 'preact';
import { onLink } from '../../router';
import { Lock } from './icons';

const TONES = {
  neutral: 'border-base-300 bg-base-200 text-base-content/70',
  primary: 'badge-soft badge-primary',
  secondary: 'badge-soft badge-secondary',
  accent: 'badge-soft badge-accent',
  info: 'badge-soft badge-info',
  success: 'badge-soft badge-success',
  warning: 'badge-soft badge-warning',
  error: 'badge-soft badge-error',
};

export type Tone = keyof typeof TONES;

/** A short label: a state, a source, a count */
export function Badge({
  tone = 'neutral',
  size = 'sm',
  class: extra = '',
  title,
  children,
}: {
  tone?: Tone;
  size?: 'xs' | 'sm';
  class?: string;
  title?: string;
  children: ComponentChildren;
}) {
  return (
    <span class={`badge ${size === 'xs' ? 'badge-xs' : 'badge-sm'} ${TONES[tone]} ${extra}`} title={title}>
      {children}
    </span>
  );
}

const STATUSES = { success: 'status-success', warning: 'status-warning', error: 'status-error', neutral: '' };

/** A dot: healthy, failing, or neither */
export function Status({ tone, title }: { tone: keyof typeof STATUSES; title?: string }) {
  return <span class={`status ${STATUSES[tone]}`} title={title} />;
}

/** A short code-like name: a block, an address, a tag. A link to a page of the UI with `href`. */
export function Chip({ href, children }: { href?: string; children: ComponentChildren }) {
  const style = 'badge badge-sm border-base-300 bg-base-200 font-mono text-base-content/70';
  return href ? (
    <a class={`${style} hover:text-base-content`} href={href} onClick={onLink}>
      {children}
    </a>
  ) : (
    <span class={style}>{children}</span>
  );
}

/**
 * What a host or block serves: HTTP requests, or TCP connections. For a host's HTTP, `tls` says
 * whether it's HTTPS, or plain HTTP (marked as a warning: nothing is encrypted).
 */
export function ProtocolBadge({ protocol, tls }: { protocol: 'http' | 'tcp'; tls?: boolean }) {
  if (protocol === 'tcp') {
    return (
      <Badge size="xs" tone="secondary" class="font-mono font-semibold">
        TCP
      </Badge>
    );
  }
  if (tls === undefined) {
    return (
      <Badge size="xs" tone="info" class="font-mono font-semibold">
        HTTP
      </Badge>
    );
  }
  return (
    <Badge
      size="xs"
      tone={tls ? 'info' : 'warning'}
      class="gap-1 font-mono font-semibold"
      title={tls ? 'HTTPS: served with TLS' : 'Plain HTTP: no TLS, nothing is encrypted'}
    >
      {tls && <Lock class="size-2.5" />}
      {tls ? 'HTTPS' : 'HTTP'}
    </Badge>
  );
}

const SOURCES: Record<string, Tone> = { store: 'primary', docker: 'info', kubernetes: 'info' };

/** Where a host comes from: the configuration file, the store, Docker or Kubernetes */
export function SourceBadge({ source }: { source: string | null }) {
  const name = source ?? 'unknown';
  return <Badge tone={SOURCES[name] ?? 'neutral'}>{name}</Badge>;
}

/** Upstreams' health: a dot each (up to 8), and how many are healthy */
export function UpstreamHealth({ upstreams }: { upstreams: { upstream: string; healthy: boolean }[] }) {
  if (upstreams.length === 0) return <span class="text-sm text-base-content/45">no upstreams</span>;
  const healthy = upstreams.filter((upstream) => upstream.healthy).length;
  return (
    <span class="inline-flex items-center gap-2 text-sm text-base-content/60">
      <span class="inline-flex gap-1">
        {upstreams.slice(0, 8).map((upstream) => (
          <Status tone={upstream.healthy ? 'success' : 'error'} title={upstream.upstream} />
        ))}
      </span>
      {healthy}/{upstreams.length} healthy
    </span>
  );
}
