import { useEffect, useState } from 'preact/hooks';
import { routes } from '../../api';
import { Alert } from '../../components/ui/Alert';
import { None, Skeleton } from '../../components/ui/Page';
import { canonical, type DiffLine, diffLines } from './versions';

/** Lines added and removed, in green and red */
const LINE = { added: 'bg-success/15', removed: 'bg-error/15', same: '' };
const SIGN = { added: 'text-success', removed: 'text-error', same: 'text-base-content/45' };

/** Two versions of a host side by side: what changed from `before` to `after` */
export function Compare({ name, before, after }: { name: string; before: number; after: number }) {
  const [lines, setLines] = useState<DiffLine[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setLines(null);
    Promise.all([routes.version(name, before), routes.version(name, after)])
      .then(([a, b]) => setLines(diffLines(canonical(a.host), canonical(b.host))))
      .catch((err) => setError(String(err.message ?? err)));
  }, [name, before, after]);

  if (error) return <Alert>{error}</Alert>;
  if (!lines) return <Skeleton width={60} />;
  if (lines.every((line) => line.kind === 'same')) {
    return <None>Versions {before} and {after} are the same.</None>;
  }
  return (
    <pre
      class="overflow-x-auto rounded-box border border-base-300 bg-base-200 py-2.5 font-mono text-xs leading-relaxed"
      aria-label={`Changes from version ${before} to ${after}`}>
      {lines.map((line) => (
        <div class={`${LINE[line.kind]} px-3 whitespace-pre`}>
          <span class={`inline-block w-[1.5ch] select-none ${SIGN[line.kind]}`}>{line.kind === 'added' ? '+' : line.kind === 'removed' ? '−' : ' '}</span>
          {line.text}
        </div>
      ))}
    </pre>
  );
}
