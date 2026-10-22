import type { ComponentChildren } from 'preact';

const TONES = { error: 'alert-error', warning: 'alert-warning', info: 'alert-info', success: 'alert-success' };

/** Something gone wrong, or worth knowing, above what it's about */
export function Alert({ tone = 'error', class: extra = '', children }: { tone?: keyof typeof TONES; class?: string; children: ComponentChildren }) {
  return (
    <div role={tone === 'error' ? 'alert' : 'status'} class={`alert alert-soft ${TONES[tone]} ${extra}`}>
      {children}
    </div>
  );
}
