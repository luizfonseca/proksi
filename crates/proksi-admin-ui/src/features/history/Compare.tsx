import { useEffect, useState } from 'preact/hooks';
import { routes } from '../../api';
import { canonical, type DiffLine, diffLines } from './versions';

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

  if (error) return <div class="error">{error}</div>;
  if (!lines) return <div class="skeleton" style={{ width: '60%' }} />;
  if (lines.every((line) => line.kind === 'same')) {
    return <p class="none">Versions {before} and {after} are the same.</p>;
  }
  return (
    <pre class="diff" aria-label={`Changes from version ${before} to ${after}`}>
      {lines.map((line) => (
        <div class={line.kind}>
          <span class="sign">{line.kind === 'added' ? '+' : line.kind === 'removed' ? '−' : ' '}</span>
          {line.text}
        </div>
      ))}
    </pre>
  );
}
