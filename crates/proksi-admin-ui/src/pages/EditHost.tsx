import { useEffect, useState } from 'preact/hooks';
import { routes } from '../api';
import {
  emptyHost,
  emptyLocation,
  emptyUpstream,
  fromForm,
  type HostForm,
  type LocationForm,
  type UpstreamForm,
  toForm,
} from '../hostForm';
import { navigate, onLink } from '../router';

/** Creates a host in the store (`name` null), or changes one */
export function EditHost({ name }: { name: string | null }) {
  const [form, setForm] = useState<HostForm | null>(name ? null : emptyHost());
  const [errors, setErrors] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!name) return;
    routes
      .get(name)
      .then(({ host }) => setForm(toForm(host)))
      .catch((err) => setErrors([String(err.message ?? err)]));
  }, [name]);

  if (!form) return errors.length ? <div class="error">{errors[0]}</div> : null;

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
      await routes.put(result.host);
      navigate(`/hosts/${encodeURIComponent(result.host.name)}`);
    } catch (err) {
      setErrors([String((err as Error).message ?? err)]);
    } finally {
      setBusy(false);
    }
  }

  const back = name ? `/hosts/${encodeURIComponent(name)}` : '/hosts';
  return (
    <form class="editor" onSubmit={save}>
      <div class="crumbs">
        <a href="/hosts" onClick={onLink}>Hosts</a> / {name ? <>{name} / edit</> : 'new host'}
      </div>
      <div class="page-head">
        <div>
          <h1>{name ?? 'New host'}</h1>
          <p>Saved to the store, and served by every instance sharing it.</p>
        </div>
        <div class="actions">
          <a class="button" href={back} onClick={onLink}>Cancel</a>
          <button class="primary" disabled={busy}>{busy ? 'Saving…' : 'Save'}</button>
        </div>
      </div>

      {errors.length > 0 && (
        <div class="error" role="alert">
          {errors.map((error) => <div>{error}</div>)}
        </div>
      )}

      <div class="panel fields">
        {!name && (
          <label>
            <span>Name</span>
            <input autoFocus placeholder="app.example.com" value={form.name} onInput={(e) => update({ name: e.currentTarget.value })} />
          </label>
        )}
        <label class="check">
          <input type="checkbox" checked={form.tls} onChange={(e) => update({ tls: e.currentTarget.checked })} />
          <span>HTTPS <small>Off: plain HTTP, without certificates</small></span>
        </label>
        {form.tls && (
          <label class="check">
            <input type="checkbox" checked={form.selfSigned} onChange={(e) => update({ selfSigned: e.currentTarget.checked })} />
            <span>Self-signed certificate if none can be obtained</span>
          </label>
        )}
      </div>

      <h2>Locations</h2>
      {form.locations.map((location, index) => (
        <div class="panel fields">
          <div class="row">
            <label class="grow">
              <span>Path</span>
              <input class="mono" value={location.path} onInput={(e) => updateLocation(index, { path: e.currentTarget.value })} />
            </label>
            <label class="check">
              <input type="checkbox" checked={location.exact} onChange={(e) => updateLocation(index, { exact: e.currentTarget.checked })} />
              <span>Only this path</span>
            </label>
            <button
              type="button"
              class="link danger"
              onClick={() => update({ locations: form.locations.filter((_, at) => at !== index) })}
            >
              Remove
            </button>
          </div>
          <div class="upstream-grid">
            <span>Upstream</span>
            <span>Weight</span>
            <span>SNI</span>
            <span />
            {location.upstreams.map((upstream, at) => (
              <>
                <input class="mono" placeholder="app.internal:8080" value={upstream.address} onInput={(e) => updateUpstream(index, at, { address: e.currentTarget.value })} />
                <input type="number" min="1" max="127" placeholder="1" value={upstream.weight} onInput={(e) => updateUpstream(index, at, { weight: e.currentTarget.value })} />
                <input placeholder="none" value={upstream.sni} onInput={(e) => updateUpstream(index, at, { sni: e.currentTarget.value })} />
                <button
                  type="button"
                  class="link danger"
                  aria-label="Remove upstream"
                  onClick={() => updateLocation(index, { upstreams: location.upstreams.filter((_, other) => other !== at) })}
                >
                  ✕
                </button>
              </>
            ))}
          </div>
          <button type="button" class="link" onClick={() => updateLocation(index, { upstreams: [...location.upstreams, emptyUpstream()] })}>
            + Add upstream
          </button>
          {Object.keys(location.rest).filter((key) => location.rest[key] != null).length > 0 && (
            <p class="kept">
              Kept as they are: {Object.keys(location.rest).filter((key) => location.rest[key] != null).join(', ')}
            </p>
          )}
        </div>
      ))}
      <button type="button" class="link" onClick={() => update({ locations: [...form.locations, emptyLocation()] })}>
        + Add location
      </button>
    </form>
  );
}
