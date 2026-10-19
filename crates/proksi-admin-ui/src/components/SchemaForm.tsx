import { useState } from 'preact/hooks';
import { input, type JsonSchema, resolve } from '../schema';

type Value = Record<string, unknown>;
type Props = { schema: JsonSchema; root?: JsonSchema; value: Value; onChange: (value: Value) => void };

/** Secrets are typed hidden */
const SECRET = /pass|secret/;

/** A description as plain text (they're written for Markdown) */
const plain = (text?: string) => text?.replace(/`/g, '').replace(/\s*\n\s*/g, ' ');

/** A list of words: typed, then added with Enter or a comma */
export function ListInput({ value, onChange, placeholder }: { value: string[]; onChange: (value: string[]) => void; placeholder?: string }) {
  const [draft, setDraft] = useState('');
  const commit = () => {
    const words = draft.split(',').map((word) => word.trim()).filter(Boolean);
    if (words.length) onChange([...value, ...words]);
    setDraft('');
  };
  return (
    <div class="list-input">
      {value.map((word, index) => (
        <span class="chip">
          {word}
          <button type="button" aria-label={`Remove ${word}`} onClick={() => onChange(value.filter((_, at) => at !== index))}>
            ✕
          </button>
        </span>
      ))}
      <input
        value={draft}
        placeholder={value.length ? '' : placeholder ?? 'Type, then Enter'}
        onInput={(e) => setDraft(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ',') {
            e.preventDefault();
            commit();
          } else if (e.key === 'Backspace' && !draft && value.length) {
            onChange(value.slice(0, -1));
          }
        }}
        onBlur={commit}
      />
    </div>
  );
}

function MapInput({ value, onChange }: { value: Record<string, string>; onChange: (value: Record<string, string>) => void }) {
  const entries = Object.entries(value);
  const set = (next: [string, string][]) => onChange(Object.fromEntries(next));
  return (
    <div class="map-input">
      {entries.map(([key, text], index) => (
        <div class="map-row">
          <input class="mono" placeholder="name" value={key} onInput={(e) => set(entries.map((entry, at) => (at === index ? [e.currentTarget.value, entry[1]] : entry)))} />
          <input class="mono" placeholder="value" value={text} onInput={(e) => set(entries.map((entry, at) => (at === index ? [entry[0], e.currentTarget.value] : entry)))} />
          <button type="button" class="link danger" aria-label="Remove" onClick={() => set(entries.filter((_, at) => at !== index))}>
            ✕
          </button>
        </div>
      ))}
      <button type="button" class="link" onClick={() => set([...entries, ['', '']])}>
        + Add
      </button>
    </div>
  );
}

/** One setting's input */
function Setting({ name, schema, root, value, onChange }: { name: string; schema: JsonSchema; root: JsonSchema; value: unknown; onChange: (value: unknown) => void }) {
  const resolved = resolve(schema, root);
  switch (input(schema, root)) {
    case 'text':
    {
      const text = (value as string) ?? '';
      const set = (e: { currentTarget: HTMLInputElement }) => onChange(e.currentTarget.value === '' ? undefined : e.currentTarget.value);
      return SECRET.test(name) ? (
        <input class="mono" type="password" value={text} onInput={set} />
      ) : (
        <input class={name === 'body' ? undefined : 'mono'} value={text} onInput={set} />
      );
    }
    case 'integer':
      return (
        <input
          type="number"
          min={resolved.minimum}
          max={resolved.maximum}
          value={(value as number | undefined) ?? ''}
          onInput={(e) => onChange(e.currentTarget.value === '' ? undefined : Number(e.currentTarget.value))}
        />
      );
    case 'boolean':
      return (
        <select value={value === undefined ? '' : String(value)} onChange={(e) => onChange(e.currentTarget.value === '' ? undefined : e.currentTarget.value === 'true')}>
          <option value="">default</option>
          <option value="true">yes</option>
          <option value="false">no</option>
        </select>
      );
    case 'choice':
      return (
        <select value={(value as string) ?? ''} onChange={(e) => onChange(e.currentTarget.value || undefined)}>
          <option value="">—</option>
          {resolved.enum?.map((option) => <option value={option}>{option}</option>)}
        </select>
      );
    case 'list':
      return <ListInput value={(value as string[]) ?? []} onChange={(next) => onChange(next.length ? next : undefined)} />;
    case 'choices': {
      const options = resolve(resolved.items ?? {}, root).enum ?? [];
      const chosen = (value as string[]) ?? [];
      return (
        <div class="choices">
          {options.map((option) => (
            <label class="check">
              <input
                type="checkbox"
                checked={chosen.includes(option)}
                onChange={(e) => {
                  const next = e.currentTarget.checked ? [...chosen, option] : chosen.filter((one) => one !== option);
                  onChange(next.length ? next : undefined);
                }}
              />
              <span class="mono">{option}</span>
            </label>
          ))}
        </div>
      );
    }
    case 'map':
      return <MapInput value={(value as Record<string, string>) ?? {}} onChange={(next) => onChange(Object.keys(next).length ? next : undefined)} />;
    case 'object':
      return (
        <div class="nested-form">
          <SchemaForm schema={resolved} root={root} value={(value as Value) ?? {}} onChange={(next) => onChange(Object.keys(next).length ? next : undefined)} />
        </div>
      );
    case 'objects': {
      const items = (value as Value[]) ?? [];
      const item = resolve(resolved.items ?? {}, root);
      const set = (next: Value[]) => onChange(next.length ? next : undefined);
      return (
        <div class="objects">
          {items.map((entry, index) => (
            <div class="nested-form">
              <SchemaForm schema={item} root={root} value={entry} onChange={(next) => set(items.map((one, at) => (at === index ? next : one)))} />
              <button type="button" class="link danger" onClick={() => set(items.filter((_, at) => at !== index))}>
                Remove
              </button>
            </div>
          ))}
          <button type="button" class="link" onClick={() => set([...items, {}])}>
            + Add
          </button>
        </div>
      );
    }
    default:
      return <span class="none">Edit as JSON</span>;
  }
}

/** A form for an object of `schema` (a block's settings), built from its JSON Schema: required
 * and set settings first, the others behind "More settings" */
export function SchemaForm({ schema, root = schema, value, onChange }: Props) {
  const [more, setMore] = useState(false);
  const required = schema.required ?? [];
  const properties = Object.entries(schema.properties ?? {}).sort(([a], [b]) => Number(required.includes(b)) - Number(required.includes(a)));
  const shown = properties.filter(([name]) => more || required.includes(name) || value[name] !== undefined);
  const hidden = properties.length - shown.length;
  return (
    <div class="schema-form">
      {shown.map(([name, property]) => (
        <label class="setting">
          <span class="setting-name">
            <span class="mono">{name}</span>
            {required.includes(name) && <span class="required">required</span>}
          </span>
          <Setting
            name={name}
            schema={property}
            root={root}
            value={value[name]}
            onChange={(next) => {
              const copy = { ...value };
              if (next === undefined) delete copy[name];
              else copy[name] = next;
              onChange(copy);
            }}
          />
          {plain(resolve(property, root).description) && <small class="hint">{plain(resolve(property, root).description)}</small>}
        </label>
      ))}
      {hidden > 0 && (
        <button type="button" class="link more-settings" onClick={() => setMore(true)}>
          More settings ({hidden})
        </button>
      )}
    </div>
  );
}
