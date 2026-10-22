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
import { Checkbox, Input } from '../components/ui/Form';
import { Hint, Mono, PageHead, PageSection, Panel, Section } from '../components/ui/Page';
import { Grid, Stack } from '../components/ui/Stack';
import { toast } from '../components/ui/Toast';

/** A scope's rules: their editor once there are some, or when asked for */
function RulesSection({ rules, onChange }: { rules: unknown[]; onChange: (rules: unknown[]) => void }) {
  const [open, setOpen] = useState(rules.length > 0);
  if (!open) {
    return (
      <Button variant="link" onClick={() => setOpen(true)}>
        + Add rules
      </Button>
    );
  }
  return <RulesEditor rules={rules} onChange={onChange} />;
}

/** How rules work, in the docs */
const RULES_DOCS = (
  <a class="hover:underline" href="https://docs.proksi.info/rules" target="_blank" rel="noreferrer">
    How rules work
  </a>
);

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
      const written = await routes.put(result.host, version === null ? 'absent' : { version });
      const page = `/hosts/${encodeURIComponent(result.host.name)}`;
      toast(
        name
          ? { message: `${result.host.name} saved as version ${written.revision}`, action: { label: 'History', href: `${page}/history` } }
          : { message: `${result.host.name} created: every instance serves it within seconds` },
      );
      navigate(page);
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

      <Panel>
        {!name && (
          <Section label="Name">
            <Input autoFocus placeholder="app.example.com" value={form.name} onInput={(e) => update({ name: e.currentTarget.value })} />
          </Section>
        )}
        <Section label="TLS">
          <Grid gap={2.5}>
            <Checkbox label="HTTPS" hint="Off: plain HTTP, without certificates" checked={form.tls} onChange={(tls) => update({ tls })} />
            {form.tls && (
              <Checkbox label="Self-signed certificate if none can be obtained" checked={form.selfSigned} onChange={(selfSigned) => update({ selfSigned })} />
            )}
          </Grid>
        </Section>
        <Section label="Host rules" hint={<>every request, before its location is chosen · {RULES_DOCS}</>}>
          <RulesSection rules={form.rules} onChange={(rules) => update({ rules })} />
        </Section>
      </Panel>

      <PageSection
        title="Locations"
        description="Chosen by the request's path: the most specific one serves it"
        aside={
          <Button onClick={() => update({ locations: [...form.locations, emptyLocation()] })}>
            + Add location
          </Button>
        }
      >
        {form.locations.map((location, index) => {
          const kept = Object.keys(location.rest).filter((key) => location.rest[key] != null);
          return (
            <Panel>
              <Stack class="border-b border-base-300 px-4 py-3">
                <Mono size="md" class="font-semibold">{location.path || 'New location'}</Mono>
                <Button variant="link-danger" class="ml-auto" onClick={() => update({ locations: form.locations.filter((_, at) => at !== index) })}>
                  Remove location
                </Button>
              </Stack>
              <Section label="Path">
                <Stack gap={4} wrap>
                  <Input mono class="max-w-md min-w-[200px] flex-1" value={location.path} onInput={(e) => updateLocation(index, { path: e.currentTarget.value })} />
                  <Checkbox label="Only this path" checked={location.exact} onChange={(exact) => updateLocation(index, { exact })} />
                </Stack>
              </Section>
              <Section label="Upstreams">
                <Grid gap={2}>
                  <div class="grid grid-cols-[minmax(160px,1fr)_80px_minmax(100px,0.6fr)_24px] items-center gap-2 max-md:grid-cols-[1fr_64px_24px] max-md:[&>:nth-child(4n+3)]:hidden">
                    <Hint>Address</Hint>
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
                </Grid>
              </Section>
              <Section label="Rules" hint={<>this location's requests, in order · {RULES_DOCS}</>}>
                <RulesSection rules={location.rules} onChange={(rules) => updateLocation(index, { rules })} />
              </Section>
              {kept.length > 0 && (
                <Section label="Other settings" hint="kept as they are">
                  <Mono>{kept.join(', ')}</Mono>
                </Section>
              )}
            </Panel>
          );
        })}
      </PageSection>
    </form>
  );
}
