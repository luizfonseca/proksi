import type { ComponentChildren } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { onLink } from '../../router';

/** What happened, and maybe a page to go to about it (`History`, `Restore`) */
export type Toast = { message: ComponentChildren; tone?: 'success' | 'info'; action?: { label: string; href: string } };

type Shown = Toast & { id: number };

/** How long a toast stays */
const SHOWN_MS = 6000;

let next = 0;
const listeners = new Set<(toast: Shown) => void>();

/** Tells what just happened (a host saved, removed, rolled back), at the bottom right for a few seconds */
export function toast(toast: Toast) {
  const shown = { ...toast, id: next++ };
  for (const listener of listeners) listener(shown);
}

/** Where toasts show: once, around the pages */
export function Toaster() {
  const [toasts, setToasts] = useState<Shown[]>([]);
  useEffect(() => {
    const add = (shown: Shown) => {
      setToasts((all) => [...all.slice(-2), shown]);
      setTimeout(() => setToasts((all) => all.filter((one) => one.id !== shown.id)), SHOWN_MS);
    };
    listeners.add(add);
    return () => {
      listeners.delete(add);
    };
  }, []);
  const dismiss = (id: number) => setToasts((all) => all.filter((one) => one.id !== id));
  return (
    <div class="toast toast-end toast-bottom z-50" role="status" aria-live="polite">
      {toasts.map((one) => (
        <div
          key={one.id}
          class="flex min-w-72 items-center gap-3 rounded-box border border-base-300 bg-base-100 px-4 py-3 text-sm shadow-xl motion-safe:animate-[toast-in_180ms_ease-out]"
        >
          <span class={`status ${one.tone === 'info' ? 'status-info' : 'status-success'}`} />
          <span class="grow">{one.message}</span>
          {one.action && (
            <a class="link link-primary text-xs font-medium no-underline hover:underline" href={one.action.href} onClick={(event) => (dismiss(one.id), onLink(event))}>
              {one.action.label}
            </a>
          )}
          <button type="button" class="cursor-pointer text-base-content/45 hover:text-base-content" aria-label="Dismiss" onClick={() => dismiss(one.id)}>
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
