import type { ComponentChildren } from 'preact';
import type { Data } from '../useData';

/** The page's content once its data is there, or what's happening until then */
export function Loaded<T>({ data, children }: { data: Data<T>; children: (value: T) => ComponentChildren }) {
  if (data.state === 'ready') return <>{children(data.value)}</>;
  if (data.state === 'error') return <div class="error">Couldn't load this: {data.message}</div>;
  return (
    <div class="panel" style={{ padding: 16, display: 'grid', gap: 12 }}>
      {[70, 55, 62].map((width) => (
        <div class="skeleton" style={{ width: `${width}%` }} />
      ))}
    </div>
  );
}

/** What a host or block serves: HTTP requests, or TCP connections */
export function ProtocolBadge({ protocol }: { protocol: 'http' | 'tcp' }) {
  return <span class={`badge protocol ${protocol}`}>{protocol.toUpperCase()}</span>;
}

export function SourceBadge({ source }: { source: string | null }) {
  const name = source ?? 'unknown';
  return <span class={`badge ${name}`}>{name}</span>;
}
