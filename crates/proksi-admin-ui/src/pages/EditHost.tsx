import type { ComponentChildren } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { Conflict, routes } from '../api';
import {
  emptyHost,
  emptyLocation,
  emptyUpstream,
  fromForm,
  type HostForm,
  type LocationForm,
  type UpstreamForm,
  toForm,
} from '../features/hosts/hostForm';
import { navigate, onLink } from '../router';
import { RulesEditor } from '../features/rules/RuleEditor';
import { Alert } from '../components/ui/Alert';
import { Button } from '../components/ui/Button';
import { Checkbox, Field, Input } from '../components/ui/Form';
import { Hint, PageHead, Panel } from '../components/ui/Page';
import { Grid } from '../components/ui/Stack';

/** A box of fields */
function Fields({ children }: { children: ComponentChildren }) {
  return <Panel class="mb-3 grid gap-3.5 p-4">{children}</Panel>;
}

/** A scope's rules: shown once there are some, or when asked for */
function RulesSection({ title, hint, rules, onChange }: { title: string; hint: string; rules: unknown[]; onChange: (rules: unknown[]) => void }) {
  const [open, setOpen] = useState(rules.length > 0);
  if (!open) {
    return (
      <Button variant="link" onClick={() => setOpen(true)}>
        + {title}
      </Button>
    );
  }
  return (
    <Grid gap={2}>
      <div class="text-sm">
        <strong>{title}</strong> <Hint class="ml-1">{hint}</Hint>{' '}
        <a class="ml-1 text-xs text-base-content/45 hover:underline" href="https://docs.proksi.info/rules" target="_blank" rel="noreferrer">
          How rules work
        </a>
      </div>
      <RulesEditor rules={rules} onChange={onChange} />
    </Grid>
  );
}

/** Creates a host in the store (`name` null), or changes one */
export function EditHost({ name }: { name: string | null }) {
  const [form, setForm] = useState<HostForm | null>(name ? null : emptyHost());
  // The version opened: saving is refused if the host is on another one by then
  const [version, setVersion] = useState<number | null>(null);
  const [errors, setErrors] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  // What Proksi says of the form as it is, checked shortly after each change
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    if (!form) return;
    const result = fromForm(form);
    if (!('host' in result)) return setProblem(null);
    const timer = setTimeout(() => {
      routes
        .validate(result.host)
        .then(() => setProblem(null))
        .catch((err) => setProblem(String(err.message ?? err)));
    }, 500);
    return () => clearTimeout(timer);
  }, [form]);

  useEffect(() => {
    if (!name) return;
    routes
      .get(name)
      .then(({ host, version }) => {
        setForm(toForm(host));
        setVersion(version);
      })
      .catch((err) => setErrors([String(err.message ?? err)]));
  }, [name]);

  if (!form) return errors.length ? <Alert>{errors[0]}</Alert> : null;

  const update = (change: Partial<HostForm>) => setForm({ ...form, ...change });
  const updateLocation = (index: number, change: Partial<LocationForm>) =>
    update({ locations: form.locations.map((location, at) => (at === index ? { ...location, ...change } : location)) });
  const updateUpstream = (index: number, upstream: number, change: Partial<UpstreamForm>) =>
    updateLocation(index, {
      upstreams: form.locations[index].upstreams.map((current, at) => (at === upstream ? { ...current, ...change } : current)),
    });

  async function save(event: Event) {
    event.preventDefault();
    const result = fromForm(form!);
    if ('errors' in result) return setErrors(result.errors);
    setBusy(true);
    try {
      await routes.put(result.host, version === null ? 'absent' : { version });
      navigate(`/hosts/${encodeURIComponent(result.host.name)}`);
    } catch (err) {
      if (err instanceof Conflict) {
        const reason = name
          ? 'Someone else changed this host since you opened it. Reload to see their change (yours would replace it).'
          : `A host named ${result.host.name} already exists.`;
        return setErrors([reason]);
      }
      setErrors([String((err as Error).message ?? err)]);
    } finally {
      setBusy(false);
    }
  }

  const back = name ? `/hosts/${encodeURIComponent(name)}` : '/hosts';
  return (
    <form onSubmit={save}>
      <PageHead
        crumbs={
          <ul>
            <li>
              <a href="/hosts" onClick={onLink}>Hosts</a>
            </li>
            {name ? (
              <>
                <li>{name}</li>
                <li>edit</li>
              </>
            ) : (
              <li>new host</li>
            )}
          </ul>
        }
        title={name ?? 'New host'}
        description="Saved to the store, and served by every instance sharing it."
        actions={
          <>
            <Button href={back}>Cancel</Button>
            <Button variant="primary" submit disabled={busy}>
              {busy ? 'Saving…' : 'Save'}
            </Button>
          </>
        }
      />

      {problem && errors.length === 0 && (
        <Alert class="mb-4">{problem}</Alert>
      )}
      {errors.length > 0 && (
        <Alert class="mb-4 grid gap-1">
          {errors.map((error) => <div>{error}</div>)}
        </Alert>
      )}

      <Fields>
        {!name && (
          <Field label="Name">
            <Input autoFocus placeholder="app.example.com" value={form.name} onInput={(e) => update({ name: e.currentTarget.value })} />
          </Field>
        )}
        <Checkbox label="HTTPS" hint="Off: plain HTTP, without certificates" checked={form.tls} onChange={(tls) => update({ tls })} />
        {form.tls && (
          <Checkbox label="Self-signed certificate if none can be obtained" checked={form.selfSigned} onChange={(selfSigned) => update({ selfSigned })} />
        )}
        <RulesSection
          title="Host rules"
          hint="every request, before its location is chosen"
          rules={form.rules}
          onChange={(rules) => update({ rules })}
        />
      </Fields>

      <h2 class="mt-6 mb-2.5 text-sm font-semibold">Locations</h2>
      {form.locations.map((location, index) => (
        <Fields>
          <div class="flex flex-wrap items-end gap-4">
            <Field label="Path" class="min-w-[200px] flex-1">
              <Input mono value={location.path} onInput={(e) => updateLocation(index, { path: e.currentTarget.value })} />
            </Field>
            <Checkbox class="pb-2" label="Only this path" checked={location.exact} onChange={(exact) => updateLocation(index, { exact })} />
            <Button variant="link-danger" class="pb-2" onClick={() => update({ locations: form.locations.filter((_, at) => at !== index) })}>
              Remove
            </Button>
          </div>
          <div class="grid grid-cols-[minmax(160px,1fr)_80px_minmax(100px,0.6fr)_24px] items-center gap-2 max-md:grid-cols-[1fr_64px_24px] max-md:[&>:nth-child(4n+3)]:hidden">
            <Hint>Upstream</Hint>
            <Hint>Weight</Hint>
            <Hint>SNI</Hint>
            <span />
            {location.upstreams.map((upstream, at) => (
              <>
                <Input mono placeholder="app.internal:8080" value={upstream.address} onInput={(e) => updateUpstream(index, at, { address: e.currentTarget.value })} />
                <Input type="number" min="1" max="127" placeholder="1" value={upstream.weight} onInput={(e) => updateUpstream(index, at, { weight: e.currentTarget.value })} />
                <Input placeholder="none" value={upstream.sni} onInput={(e) => updateUpstream(index, at, { sni: e.currentTarget.value })} />
                <Button
                  variant="link-danger"
                  aria-label="Remove upstream"
                  onClick={() => updateLocation(index, { upstreams: location.upstreams.filter((_, other) => other !== at) })}
                >
                  ✕
                </Button>
              </>
            ))}
          </div>
          <Button variant="link" onClick={() => updateLocation(index, { upstreams: [...location.upstreams, emptyUpstream()] })}>
            + Add upstream
          </Button>
          <RulesSection title="Rules" hint="this location's requests, in order" rules={location.rules} onChange={(rules) => updateLocation(index, { rules })} />
          {Object.keys(location.rest).filter((key) => location.rest[key] != null).length > 0 && (
            <Hint>Kept as they are: {Object.keys(location.rest).filter((key) => location.rest[key] != null).join(', ')}</Hint>
          )}
        </Fields>
      ))}
      <Button variant="link" onClick={() => update({ locations: [...form.locations, emptyLocation()] })}>
        + Add location
      </Button>
    </form>
  );
}
