import type { ComponentChildren } from 'preact';

/** A column: its heading, and whether phones show it */
export type Column = string | { label: ComponentChildren; phone?: boolean; class?: string };

const HEIGHTS = { none: '', 420: 'max-h-[420px]' };

/**
 * Rows under headings, scrolling sideways when they don't fit. `height`: the most it takes before
 * scrolling down, with its headings pinned.
 */
export function Table({ columns, height = 'none', children }: { columns: Column[]; height?: keyof typeof HEIGHTS; children: ComponentChildren }) {
  const pinned = height !== 'none';
  return (
    // Rounded as the panel it's in, where it meets the panel's top or bottom
    <div class={`overflow-x-auto first:rounded-t-[inherit] last:rounded-b-[inherit] ${HEIGHTS[height]}`}>
      <table class={`table ${pinned ? 'table-pin-rows' : ''}`}>
        <thead>
          <tr class="text-xs text-base-content/60 [&>th]:font-medium">
            {columns.map((column) =>
              typeof column === 'string' ? (
                <th>{column}</th>
              ) : (
                <th class={`${column.phone === false ? 'max-md:hidden' : ''} ${column.class ?? ''}`}>{column.label}</th>
              ),
            )}
          </tr>
        </thead>
        <tbody>{children}</tbody>
      </table>
    </div>
  );
}

/** A row, opening something when clicked with `onClick`; `top` aligns its cells to their top */
export function Row({
  onClick,
  top,
  class: extra = '',
  children,
}: {
  onClick?: () => void;
  top?: boolean;
  class?: string;
  children: ComponentChildren;
}) {
  const style = `${onClick ? 'row-hover cursor-pointer' : ''} ${top ? '[&>td]:align-top' : ''} ${extra}`;
  return (
    <tr class={style} onClick={onClick}>
      {children}
    </tr>
  );
}

/** A cell; `phone={false}` hides it on phones, as its column */
export function Cell({
  phone = true,
  numeric,
  span,
  class: extra = '',
  children,
}: {
  phone?: boolean;
  numeric?: boolean;
  /** Columns it spans */
  span?: number;
  class?: string;
  children?: ComponentChildren;
}) {
  return (
    <td class={`${phone ? '' : 'max-md:hidden'} ${numeric ? 'tabular-nums' : ''} ${extra}`} colSpan={span}>
      {children}
    </td>
  );
}
