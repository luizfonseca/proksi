import type { ComponentChildren, HTMLAttributes } from 'preact';
import { onLink } from '../../router';

/** A box on the page (daisyUI's colors, with a border) */
export const PANEL = 'rounded-box border border-base-300 bg-base-100';

/** A page's title, what it is, and what can be done from it */
export function PageHead({
  title,
  description,
  crumbs,
  badges,
  actions,
}: {
  title: ComponentChildren;
  description?: ComponentChildren;
  crumbs?: ComponentChildren;
  badges?: ComponentChildren;
  actions?: ComponentChildren;
}) {
  return (
    <div class="mb-5">
      {crumbs && <div class="breadcrumbs py-0 text-sm text-base-content/60">{crumbs}</div>}
      <div class="flex items-end gap-4">
        <div class="min-w-0">
          <h1 class="text-2xl font-semibold">{title}</h1>
          {description && <p class="mt-0.5 text-base-content/60">{description}</p>}
          {badges && <div class="mt-1.5 flex flex-wrap items-center gap-1.5">{badges}</div>}
        </div>
        {actions && <div class="ml-auto flex shrink-0 flex-wrap justify-end gap-2">{actions}</div>}
      </div>
    </div>
  );
}

/** A group of panels under a heading, with what it is and what can be done beside it */
export function PageSection({
  title,
  description,
  aside,
  children,
}: {
  title: ComponentChildren;
  description?: ComponentChildren;
  aside?: ComponentChildren;
  children: ComponentChildren;
}) {
  return (
    <section class="mt-6 first:mt-0">
      <div class="mb-2.5 flex flex-wrap items-end gap-x-4 gap-y-1">
        <div class="min-w-0">
          <h2 class="text-base font-semibold">{title}</h2>
          {description && <p class="text-sm text-base-content/60">{description}</p>}
        </div>
        {aside && <div class="ml-auto">{aside}</div>}
      </div>
      {/* One column, as wide as the page: wider content scrolls inside its panel */}
      <div class="grid grid-cols-1 gap-3">{children}</div>
    </section>
  );
}

/** A box with an optional heading row */
export function Panel({
  title,
  aside,
  class: extra = '',
  children,
  ...rest
}: {
  title?: ComponentChildren;
  aside?: ComponentChildren;
  class?: string;
  children?: ComponentChildren;
} & Omit<HTMLAttributes<HTMLElement>, 'title' | 'class'>) {
  return (
    <section class={`${PANEL} ${extra}`} {...rest}>
      {(title || aside) && (
        <div class="flex flex-wrap items-center justify-between gap-3 px-4 pt-3.5 pb-2.5">
          {title && <h3 class="text-sm font-semibold">{title}</h3>}
          {aside}
        </div>
      )}
      {children}
    </section>
  );
}

/** A part of a panel: its name in a column on the left (above, on phones), and what it holds */
export function Section({ label, hint, children }: { label: ComponentChildren; hint?: ComponentChildren; children: ComponentChildren }) {
  return (
    <div class="grid gap-2 border-t border-base-300 px-4 py-3.5 first:border-t-0 md:grid-cols-[8rem_minmax(0,1fr)] md:gap-4">
      <div class="text-xs font-medium text-base-content/60 md:pt-0.5">
        {label}
        {hint && <div class="mt-0.5 font-normal text-base-content/45">{hint}</div>}
      </div>
      <div class="min-w-0">{children}</div>
    </div>
  );
}

/** A small heading in capitals, above a part of a panel */
export function Label({ children }: { children: ComponentChildren }) {
  return <h4 class="mb-2 text-xs font-semibold uppercase tracking-wider text-base-content/45">{children}</h4>;
}

/** Nothing to show, and why */
export function Empty({ title, children, compact }: { title: ComponentChildren; children?: ComponentChildren; compact?: boolean }) {
  return (
    <div class={`${compact ? 'py-7' : 'py-12'} px-4 text-center text-base-content/60`}>
      <strong class="mb-1 block font-semibold text-base-content">{title}</strong>
      {children}
    </div>
  );
}

/** A value missing, or nothing there */
export function None({ children }: { children: ComponentChildren }) {
  return <span class="text-sm text-base-content/45">{children}</span>;
}

/** Small text that explains, next to what it's about (`block`: on a line of its own) */
export function Hint({ block, title, class: extra = '', children }: { block?: boolean; title?: string; class?: string; children: ComponentChildren }) {
  const style = `text-xs font-normal text-base-content/45 ${extra}`;
  return block ? (
    <p class={`m-0 ${style}`} title={title}>
      {children}
    </p>
  ) : (
    <small class={style} title={title}>
      {children}
    </small>
  );
}

const MONO = { sm: 'text-xs', md: 'text-sm' };

/** An address, a path, a name as written in the configuration */
export function Mono({ size = 'sm', class: extra = '', children }: { size?: keyof typeof MONO; class?: string; children: ComponentChildren }) {
  return <span class={`font-mono ${MONO[size]} ${extra}`}>{children}</span>;
}

/** A line of something still loading, `width` percent wide */
export function Skeleton({ width }: { width: number }) {
  return <div class="skeleton h-3.5" style={{ width: `${width}%` }} />;
}

/** A host's name, opening its page (grey when it's `quiet`: no traffic) */
export function HostLink({ host, quiet }: { host: string; quiet?: boolean }) {
  return (
    <a
      class={`${quiet ? 'font-medium text-base-content/60' : 'font-semibold text-base-content'} hover:text-primary`}
      href={`/hosts/${encodeURIComponent(host)}`}
      onClick={onLink}
    >
      {host}
    </a>
  );
}
