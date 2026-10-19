import { useState } from 'preact/hooks';
import { session, type Session, Unauthorized } from '../api';

export function SignIn({ onSignedIn }: { onSignedIn: (session: Session) => void }) {
  const [value, setValue] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: Event) {
    event.preventDefault();
    setBusy(true);
    try {
      onSignedIn(await session.signIn(value.trim()));
    } catch (err) {
      setError(err instanceof Unauthorized ? 'That token is not valid.' : String(err));
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
          Use the <code>secret</code> of a <code>token</code> block in your <code>admin</code> configuration.
        </p>
        <form onSubmit={submit}>
          <input type="password" autoFocus placeholder="Token" value={value} onInput={(e) => setValue(e.currentTarget.value)} />
          {error && <div class="error">{error}</div>}
          <button class="primary" disabled={busy || !value.trim()}>Sign in</button>
        </form>
      </div>
    </div>
  );
}
