// How `/api/v1/server` reads on the Server page: every setting, labelled from its name

export type Setting = string | number | boolean | null | Setting[] | { [key: string]: Setting };

const WORDS: Record<string, string> = {
  lets: "Let's",
  encrypt: 'Encrypt',
  tls: 'TLS',
  http: 'HTTP',
  https: 'HTTPS',
  mb: 'MB',
  sni: 'SNI',
  secs: '',
};

/** `shutdown_timeout_secs` → `Shutdown timeout` */
export function label(key: string): string {
  const words = key
    .split('_')
    .map((word) => WORDS[word] ?? word)
    .filter(Boolean);
  const text = words.join(' ');
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** A leaf value as text (`null` reads as `default` for upstream timeouts, `none` otherwise) */
export function value(key: string, setting: Setting): string {
  if (setting === null) return 'none';
  if (typeof setting === 'boolean') return setting ? 'on' : 'off';
  if (typeof setting === 'number' && key.endsWith('_secs')) return `${setting}s`;
  if (typeof setting === 'number' && key.endsWith('_mb')) return `${setting} MB`;
  return String(setting);
}

/** Whether a setting is a section of its own (an object), rather than a value */
export function isSection(setting: Setting): setting is { [key: string]: Setting } {
  return typeof setting === 'object' && setting !== null && !Array.isArray(setting);
}
