import { useState } from 'preact/hooks';
import { session, type Session, Unauthorized } from '../api';
import { Alert } from '../components/ui/Alert';
import { Button } from '../components/ui/Button';
import { Input } from '../components/ui/Form';
import { Panel } from '../components/ui/Page';

export function SignIn({ onSignedIn }: { onSignedIn: (session: Session) => void }) {
  const [name, setName] = useState('');
  const [value, setValue] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: Event) {
    event.preventDefault();
    setBusy(true);
    try {
      onSignedIn(await session.signIn(name.trim(), value.trim()));
    } catch (err) {
      setError(err instanceof Unauthorized ? 'That name and secret do not match a token.' : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div class="grid min-h-screen place-items-center p-6">
      <Panel class="w-full max-w-[380px] p-7">
        <img class="w-10" src="/logo.svg" alt="" />
        <h1 class="mt-3.5 mb-1 text-lg font-semibold">Sign in to Proksi</h1>
        <p class="mb-4 text-sm text-base-content/60">
          Use the name and <code>secret</code> of a <code>token</code> block in your <code>admin</code> configuration.
        </p>
        <form class="grid gap-2.5" onSubmit={submit}>
          <Input autoFocus autoComplete="username" placeholder="Name" value={name} onInput={(e) => setName(e.currentTarget.value)} />
          <Input type="password" autoComplete="current-password" placeholder="Secret" value={value} onInput={(e) => setValue(e.currentTarget.value)} />
          {error && <Alert>{error}</Alert>}
          <Button variant="primary" size="md" submit disabled={busy || !name.trim() || !value.trim()}>
            Sign in
          </Button>
        </form>
      </Panel>
    </div>
  );
}
