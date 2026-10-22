import type { ComponentChildren } from 'preact';
import { useEffect, useRef } from 'preact/hooks';

/** A panel sliding in from the right, over the page: open while `open`, closed by Escape or a click beside it */
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
    <dialog ref={dialog} class="modal modal-end" onClose={onClose}>
      <div class="modal-box flex w-full max-w-md flex-col p-0">
        <div class="flex items-center justify-between gap-3 border-b border-base-300 px-5 py-4">
          <h2 class="font-semibold">{title}</h2>
          <form method="dialog">
            <button class="btn btn-ghost btn-sm btn-square" aria-label="Close">
              ✕
            </button>
          </form>
        </div>
        <div class="grow overflow-y-auto px-5 py-4">{children}</div>
        {footer && <div class="flex justify-end gap-2 border-t border-base-300 px-5 py-4">{footer}</div>}
      </div>
      <form method="dialog" class="modal-backdrop">
        <button aria-label="Close">close</button>
      </form>
    </dialog>
  );
}

/** Names and their values, one per line */
export function Details({ items }: { items: [ComponentChildren, ComponentChildren][] }) {
  return (
    <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-6 gap-y-3 text-sm">
      {items.map(([name, value]) => (
        <>
          <dt class="text-base-content/60">{name}</dt>
          <dd class="min-w-0 break-words">{value}</dd>
        </>
      ))}
    </dl>
  );
}
