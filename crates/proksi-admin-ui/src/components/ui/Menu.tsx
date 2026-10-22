import { useState } from 'preact/hooks';
import { Button } from './Button';
import { Hint, Mono } from './Page';

export type Item = { title: string; detail?: string; onSelect: () => void };

/** A button opening a list to pick from: each item's name, and what it does */
export function Dropdown({ label, items }: { label: string; items: Item[] }) {
  const [open, setOpen] = useState(false);
  return (
    <div
      class="relative justify-self-start"
      onKeyDown={(e) => e.key === 'Escape' && setOpen(false)}
      onFocusOut={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as globalThis.Node | null)) setOpen(false);
      }}
    >
      <Button aria-expanded={open} onClick={() => setOpen(!open)}>
        {label}
      </Button>
      {open && (
        <ul
          role="menu"
          class="menu absolute top-[calc(100%+6px)] left-0 z-10 max-h-[340px] min-w-80 flex-nowrap overflow-auto rounded-box border border-base-300 bg-base-100 p-1.5 shadow-xl"
        >
          {items.map((item) => (
            <li>
              <button
                type="button"
                class="grid grid-flow-row gap-0.5 text-left"
                role="menuitem"
                onClick={() => {
                  item.onSelect();
                  setOpen(false);
                }}
              >
                <Mono class="font-semibold">{item.title}</Mono>
                {item.detail && <Hint>{item.detail}</Hint>}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
