import { useEffect, useState } from 'preact/hooks';
import { Conflict, get, type Notifications, notifications as api } from '../api';
import { Alert } from '../components/ui/Alert';
import { Button } from '../components/ui/Button';
import { Checkbox, Field, Input, ListInput, Segmented } from '../components/ui/Form';
import { Hint, PageHead, Panel, Section } from '../components/ui/Page';
import { Grid, Stack } from '../components/ui/Stack';
import { toast } from '../components/ui/Toast';
import { emptyNotify, fromForm, type HeaderForm, type NotifyForm, toForm } from '../features/notify/notifyForm';
import { navigate, onLink } from '../router';

const FORMATS: { value: NotifyForm['format']; label: string }[] = [
  { value: 'json', label: 'JSON' },
  { value: 'slack', label: 'Slack' },
  { value: 'discord', label: 'Discord' },
];

/** Creates a `notify` block in the store (`name` null), or changes one */
export function EditNotification({ name }: { name: string | null }) {
  const [form, setForm] = useState<NotifyForm | null>(name ? null : emptyNotify());
  const [events, setEvents] = useState<string[]>([]);
  // The version opened: saving is refused if the block is on another one by then
  const [version, setVersion] = useState<number | null>(null);
  // The saved URL, masked
  const [url, setUrl] = useState<string | null>(null);
  const [errors, setErrors] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    get<Notifications>('/notifications')
      .then((data) => {
        setEvents(data.events);
        if (!name) return;
        const block = data.notifications.find((candidate) => candidate.name === name && candidate.source === 'store');
        if (!block) return setErrors([`No notify block named ${name} in the store`]);
        setForm(toForm(block));
        setVersion(block.version);
        setUrl(block.url);
      })
      .catch((err) => setErrors([String(err.message ?? err)]));
  }, [name]);

  if (!form) return errors.length ? <Alert>{errors[0]}</Alert> : null;

  const update = (change: Partial<NotifyForm>) => setForm({ ...form, ...change });
  const updateHeader = (index: number, change: Partial<HeaderForm>) =>
    update({ headers: form.headers.map((header, at) => (at === index ? { ...header, ...change } : header)) });
  const toggle = (event: string, on: boolean) =>
    update({ on: on ? [...form.on, event] : form.on.filter((one) => one !== event) });

  async function save(submitted: Event) {
    submitted.preventDefault();
    const result = fromForm(form!, name !== null);
    if ('errors' in result) return setErrors(result.errors);
    setBusy(true);
    try {
      await api.put(result.name, result.body, version === null ? 'absent' : { version });
      toast({ message: name ? `${result.name} saved` : `${result.name} created: every instance uses it within seconds` });
      navigate('/notifications');
    } catch (err) {
      if (err instanceof Conflict) {
        const reason = name
          ? 'Someone else changed this block since you opened it. Reload to see their change (yours would replace it).'
          : `A notify block named ${result.name} already exists.`;
        return setErrors([reason]);
      }
      setErrors([String((err as Error).message ?? err)]);
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={save}>
      <PageHead
        crumbs={
          <ul>
            <li><a href="/notifications" onClick={onLink}>Notifications</a></li>
            <li>{name ? `${name} · edit` : 'new'}</li>
          </ul>
        }
        title={name ?? 'New notification'}
        description="Saved to the store, and used by every instance sharing it."
        actions={
          <>
            <Button href="/notifications">Cancel</Button>
            <Button variant="primary" submit disabled={busy}>
              {busy ? 'Saving…' : 'Save'}
            </Button>
          </>
        }
      />
      {errors.length > 0 && (
        <Alert class="mb-4">
          {errors.map((error) => <div>{error}</div>)}
        </Alert>
      )}
      <Panel>
        <Section label="Webhook">
          <Grid columns={2}>
            {!name && (
              <Field label="Name" hint="letters, digits, - _ .">
                <Input mono required value={form.name} placeholder="ops" onInput={(e) => update({ name: e.currentTarget.value })} />
              </Field>
            )}
            <Field label="URL" hint={url ? `saved: ${url}` : undefined}>
              <Input
                mono
                type="url"
                autoComplete="off"
                value={form.url}
                placeholder={url ? 'Leave empty to keep the saved one' : 'https://hooks.slack.com/services/…'}
                onInput={(e) => update({ url: e.currentTarget.value })}
              />
            </Field>
          </Grid>
          <div class="mt-3">
            <Field label="Format" class="w-fit">
              <Segmented options={FORMATS} value={form.format} onChange={(format) => update({ format })} label="Format" />
            </Field>
          </div>
        </Section>
        <Section label="Events" hint="none ticked: every event">
          <div class="grid gap-1.5 sm:grid-cols-2 lg:grid-cols-3">
            {events.map((event) => (
              <Checkbox neutral label={<span class="font-mono text-xs">{event}</span>} checked={form.on.includes(event)} onChange={(on) => toggle(event, on)} />
            ))}
          </div>
          {(form.on.length === 0 || form.on.includes('certificate_expiring')) && (
            <div class="mt-3 max-w-56">
              <Field label="Certificates expiring" hint="days left (1 to 60)">
                <Input type="number" min={1} max={60} value={form.expiring_days} onInput={(e) => update({ expiring_days: e.currentTarget.value })} />
              </Field>
            </div>
          )}
        </Section>
        <Section label="Hosts" hint="none: every host">
          <ListInput value={form.hosts} onChange={(hosts) => update({ hosts })} placeholder="api.example.com, *.example.com" />
          <Hint block class="mt-1.5">Events not about a host (the store, an instance, the configuration) are always sent.</Hint>
        </Section>
        <Section label="Headers" hint="sent with each event">
          <div class="grid gap-2">
            {form.headers.map((header, index) => (
              <Stack gap={2}>
                <Input mono class="max-w-56" placeholder="authorization" value={header.name} disabled={header.saved} onInput={(e) => updateHeader(index, { name: e.currentTarget.value })} />
                <Input
                  mono
                  type="password"
                  autoComplete="off"
                  placeholder={header.saved ? 'Leave empty to keep the saved value' : 'Bearer …'}
                  value={header.value}
                  onInput={(e) => updateHeader(index, { value: e.currentTarget.value })}
                />
                <Button variant="link-danger" onClick={() => update({ headers: form.headers.filter((_, at) => at !== index) })}>
                  Remove
                </Button>
              </Stack>
            ))}
            <Button variant="link" onClick={() => update({ headers: [...form.headers, { name: '', value: '' }] })}>
              + Add a header
            </Button>
          </div>
        </Section>
      </Panel>
    </form>
  );
}
