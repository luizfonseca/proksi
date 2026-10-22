import type { ComponentChildren } from 'preact';
import { useEffect, useRef } from 'preact/hooks';

/** As wide as the page it's over */
const ROW = 'mx-auto w-full max-w-7xl px-4 md:px-6';

/** A panel sliding up from the bottom, over the page: open while `open`, closed by Escape or a click above it */
export function Drawer({
  open,
  title,
  onClose,
  footer,
  children,
}: {
  open: boolean;
  title: ComponentChildren;
  onClose: () => void;
  /** Actions, at its bottom */
  footer?: ComponentChildren;
  children: ComponentChildren;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    if (open && element && !element.open) element.showModal();
    if (!open && element?.open) element.close();
  }, [open]);
  return (
    <dialog ref={dialog} class="modal modal-bottom backdrop-blur-[2px]" onClose={onClose}>
      <div class="modal-box flex max-h-[70vh] flex-col p-0">
        <div class="border-b border-base-300 py-4">
          <div class={`${ROW} flex items-center justify-between gap-3`}>
            <h2 class="font-semibold">{title}</h2>
            <form method="dialog">
              <button class="btn btn-ghost btn-sm btn-square" aria-label="Close">
                ✕
              </button>
            </form>
          </div>
        </div>
        <div class="grow overflow-y-auto py-5">
          <div class={ROW}>{children}</div>
        </div>
        {footer && (
          <div class="border-t border-base-300 py-4">
            <div class={`${ROW} flex justify-end gap-2`}>{footer}</div>
          </div>
        )}
      </div>
      <form method="dialog" class="modal-backdrop">
        <button aria-label="Close">close</button>
      </form>
    </dialog>
  );
}

/** Names and their values: one pair per line, two from tablets up */
export function Details({ items }: { items: [ComponentChildren, ComponentChildren][] }) {
  return (
    <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-6 gap-y-3 text-sm md:grid-cols-[auto_minmax(0,1fr)_auto_minmax(0,1fr)]">
      {items.map(([name, value]) => (
        <>
          <dt class="text-base-content/60">{name}</dt>
          <dd class="min-w-0 break-words">{value}</dd>
        </>
      ))}
    </dl>
  );
}
