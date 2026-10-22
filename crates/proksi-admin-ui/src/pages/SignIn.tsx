import { useState } from 'preact/hooks';
import { session, type Session, Unauthorized } from '../api';

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
    <div class="signin">
      <div class="panel">
        <img src="/logo.svg" alt="" />
        <h1>Sign in to Proksi</h1>
        <p>
          Use the name and <code>secret</code> of a <code>token</code> block in your <code>admin</code> configuration.
        </p>
        <form onSubmit={submit}>
          <input autoFocus autoComplete="username" placeholder="Name" value={name} onInput={(e) => setName(e.currentTarget.value)} />
          <input type="password" autoComplete="current-password" placeholder="Secret" value={value} onInput={(e) => setValue(e.currentTarget.value)} />
          {error && <div class="error">{error}</div>}
          <button class="primary" disabled={busy || !name.trim() || !value.trim()}>Sign in</button>
        </form>
      </div>
    </div>
  );
}
