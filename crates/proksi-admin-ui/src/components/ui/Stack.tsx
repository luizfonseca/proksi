import type { ComponentChildren } from 'preact';

// Fixed class names, so the built CSS has only those used

const GAPS = { 1: 'gap-1', 1.5: 'gap-1.5', 2: 'gap-2', 2.5: 'gap-2.5', 3: 'gap-3', 4: 'gap-4' };
const ALIGN = { start: 'items-start', center: 'items-center', end: 'items-end', baseline: 'items-baseline', stretch: '' };
const JUSTIFY = { start: '', between: 'justify-between', end: 'justify-end' };

type Gap = keyof typeof GAPS;

/** Things in a row (or a column, with `column`), spaced by `gap` */
export function Stack({
  column,
  gap = 2,
  align = 'center',
  justify = 'start',
  wrap,
  class: extra = '',
  children,
}: {
  column?: boolean;
  gap?: Gap;
  align?: keyof typeof ALIGN;
  justify?: keyof typeof JUSTIFY;
  wrap?: boolean;
  class?: string;
  children: ComponentChildren;
}) {
  const style = `flex ${column ? 'flex-col' : ''} ${wrap ? 'flex-wrap' : ''} ${GAPS[gap]} ${ALIGN[align]} ${JUSTIFY[justify]} ${extra}`;
  return <div class={style}>{children}</div>;
}

/** Columns: one on phones, `columns` from tablets up */
const COLUMNS = {
  1: '',
  2: 'md:grid-cols-2',
  3: 'md:grid-cols-3',
};

/** Things in columns, spaced by `gap` */
export function Grid({
  columns = 1,
  gap = 3,
  class: extra = '',
  children,
}: {
  columns?: keyof typeof COLUMNS;
  gap?: Gap;
  class?: string;
  children: ComponentChildren;
}) {
  return <div class={`grid grid-cols-1 ${COLUMNS[columns]} ${GAPS[gap]} ${extra}`}>{children}</div>;
}
