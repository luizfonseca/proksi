import { useEffect, useState } from 'preact/hooks';
import { relative } from '../../lib/format';

/** Now, again every `every` milliseconds: for what says how long ago something was */
function useNow(every: number): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), every);
    return () => clearInterval(timer);
  }, [every]);
  return now;
}

/** A time (`at`, in milliseconds) as how long ago it was, the exact time when hovered (`exact`: shown too) */
export function Time({ at, exact, class: extra = '' }: { at: number; exact?: boolean; class?: string }) {
  const now = useNow(30_000);
  const date = new Date(at);
  return (
    <time class={extra} dateTime={date.toISOString()} title={date.toLocaleString()}>
      {relative(at, now)}
      {exact && <span class="text-base-content/45"> · {date.toLocaleTimeString()}</span>}
    </time>
  );
}
