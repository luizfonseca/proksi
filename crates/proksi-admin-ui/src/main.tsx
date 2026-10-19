import { render } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { session, type Session, Unauthorized } from './api';
import { Layout } from './components/Layout';
import { Certificates } from './pages/Certificates';
import { Host } from './pages/Host';
import { Hosts } from './pages/Hosts';
import { SignIn } from './pages/SignIn';
import { navigate, usePath } from './router';
import './styles.css';

function Page({ path }: { path: string }) {
  if (path.startsWith('/hosts/')) return <Host name={decodeURIComponent(path.slice('/hosts/'.length))} />;
  if (path === '/certificates') return <Certificates />;
  return <Hosts />;
}

function App() {
  const path = usePath();
  // undefined: checking; null: signed out
  const [signedIn, setSignedIn] = useState<Session | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    session
      .get()
      .then(setSignedIn)
      .catch((err) => (err instanceof Unauthorized ? setSignedIn(null) : setError(String(err))));
  }, []);
  useEffect(() => {
    if (path === '/') navigate('/hosts');
  }, [path]);

  if (error) return <div class="signin"><div class="panel error">{error}</div></div>;
  if (signedIn === undefined) return null;
  if (signedIn === null) return <SignIn onSignedIn={setSignedIn} />;
  return (
    <Layout path={path} session={signedIn} onSignOut={() => setSignedIn(null)}>
      <Page path={path} />
    </Layout>
  );
}

render(<App />, document.getElementById('app')!);
