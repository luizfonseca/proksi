import type { ComponentChildren } from 'preact';
import type { Data } from '../../api/useData';
import { Alert } from './Alert';
import { PANEL, Skeleton } from './Page';

/** The page's content once its data is there, or what's happening until then */
export function Loaded<T>({ data, children }: { data: Data<T>; children: (value: T) => ComponentChildren }) {
  if (data.state === 'ready') return <>{children(data.value)}</>;
  if (data.state === 'error') return <Alert>Couldn't load this: {data.message}</Alert>;
  return (
    <div class={`${PANEL} grid gap-3 p-4`}>
      {[70, 55, 62].map((width) => (
        <Skeleton width={width} />
      ))}
    </div>
  );
}
