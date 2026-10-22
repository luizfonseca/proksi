import type { ComponentChildren } from 'preact';
import { useState } from 'preact/hooks';
import { asList } from '../../lib/lists';
import { Button } from '../../components/ui/Button';
import { Checkbox, Input, ListInput, Select } from '../../components/ui/Form';
import { Mono, None } from '../../components/ui/Page';
import { Grid } from '../../components/ui/Stack';
import { input, type JsonSchema, resolve } from './schema';

type Value = Record<string, unknown>;
type Props = { schema: JsonSchema; root?: JsonSchema; value: Value; onChange: (value: Value) => void };

/** Secrets are typed hidden */
const SECRET = /pass|secret/;

/** A description as plain text (they're written for Markdown) */
const plain = (text?: string) => text?.replace(/`/g, '').replace(/\s*\n\s*/g, ' ');

function MapInput({ value, onChange }: { value: Record<string, string>; onChange: (value: Record<string, string>) => void }) {
  const entries = Object.entries(value);
  const set = (next: [string, string][]) => onChange(Object.fromEntries(next));
  return (
    <Grid gap={2}>
      {entries.map(([key, text], index) => (
        <div class="grid grid-cols-[1fr_1fr_20px] items-center gap-1.5">
          <Input mono placeholder="name" value={key} onInput={(e) => set(entries.map((entry, at) => (at === index ? [e.currentTarget.value, entry[1]] : entry)))} />
          <Input mono placeholder="value" value={text} onInput={(e) => set(entries.map((entry, at) => (at === index ? [entry[0], e.currentTarget.value] : entry)))} />
          <Button variant="link-danger" aria-label="Remove" onClick={() => set(entries.filter((_, at) => at !== index))}>
            ✕
          </Button>
        </div>
      ))}
      <Button variant="link" onClick={() => set([...entries, ['', '']])}>
        + Add
      </Button>
    </Grid>
  );
}

/** Settings of a setting, with a line down their side */
function Nested({ children }: { children: ComponentChildren }) {
  return <div class="grid gap-1.5 border-l-2 border-base-300 pl-2.5">{children}</div>;
}

/** One setting's input */
function Setting({ name, schema, root, value, onChange }: { name: string; schema: JsonSchema; root: JsonSchema; value: unknown; onChange: (value: unknown) => void }) {
  const resolved = resolve(schema, root);
  switch (input(schema, root)) {
    case 'text':
    {
      const text = (value as string) ?? '';
      const set = (e: { currentTarget: HTMLInputElement }) => onChange(e.currentTarget.value === '' ? undefined : e.currentTarget.value);
      return SECRET.test(name) ? <Input mono type="password" value={text} onInput={set} /> : <Input mono={name !== 'body'} value={text} onInput={set} />;
    }
    case 'integer':
      return (
        <Input
          type="number"
          min={resolved.minimum}
          max={resolved.maximum}
          value={(value as number | undefined) ?? ''}
          onInput={(e) => onChange(e.currentTarget.value === '' ? undefined : Number(e.currentTarget.value))}
        />
      );
    case 'boolean':
      return (
        <Select class="self-start" value={value === undefined ? '' : String(value)} onChange={(e) => onChange(e.currentTarget.value === '' ? undefined : e.currentTarget.value === 'true')}>
          <option value="">default</option>
          <option value="true">yes</option>
          <option value="false">no</option>
        </Select>
      );
    case 'choice':
      return (
        <Select class="self-start" value={(value as string) ?? ''} onChange={(e) => onChange(e.currentTarget.value || undefined)}>
          <option value="">—</option>
          {resolved.enum?.map((option) => <option value={option}>{option}</option>)}
        </Select>
      );
    case 'list':
      return <ListInput value={asList(value)} onChange={(next) => onChange(next.length ? next : undefined)} />;
    case 'choices': {
      const options = resolve(resolved.items ?? {}, root).enum ?? [];
      const chosen = asList(value);
      return (
        <div class="flex flex-wrap gap-3.5">
          {options.map((option) => (
            <Checkbox
              label={<Mono>{option}</Mono>}
              checked={chosen.includes(option)}
              onChange={(checked) => {
                const next = checked ? [...chosen, option] : chosen.filter((one) => one !== option);
                onChange(next.length ? next : undefined);
              }}
            />
          ))}
        </div>
      );
    }
    case 'map':
      return <MapInput value={(value as Record<string, string>) ?? {}} onChange={(next) => onChange(Object.keys(next).length ? next : undefined)} />;
    case 'object':
      return (
        <Nested>
          <SchemaForm schema={resolved} root={root} value={(value as Value) ?? {}} onChange={(next) => onChange(Object.keys(next).length ? next : undefined)} />
        </Nested>
      );
    case 'objects': {
      const items = (value as Value[]) ?? [];
      const item = resolve(resolved.items ?? {}, root);
      const set = (next: Value[]) => onChange(next.length ? next : undefined);
      return (
        <Grid gap={2}>
          {items.map((entry, index) => (
            <Nested>
              <SchemaForm schema={item} root={root} value={entry} onChange={(next) => set(items.map((one, at) => (at === index ? next : one)))} />
              <Button variant="link-danger" onClick={() => set(items.filter((_, at) => at !== index))}>
                Remove
              </Button>
            </Nested>
          ))}
          <Button variant="link" onClick={() => set([...items, {}])}>
            + Add
          </Button>
        </Grid>
      );
    }
    default:
      return <None>Edit as JSON</None>;
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
    <Grid gap={2.5}>
      {shown.map(([name, property]) => (
        <label class="flex flex-col gap-1 text-xs">
          <span class="flex items-center gap-1.5 text-base-content/60">
            <span class="font-mono">{name}</span>
            {required.includes(name) && <span class="text-xs text-primary">required</span>}
          </span>
          {plain(resolve(property, root).description) && (
            <small class="text-xs leading-snug text-base-content/45">{plain(resolve(property, root).description)}</small>
          )}
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
        </label>
      ))}
      {hidden > 0 && (
        <Button variant="link" onClick={() => setMore(true)}>
          More settings ({hidden})
        </Button>
      )}
    </Grid>
  );
}
