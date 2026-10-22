import { render } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { session, type Session, Unauthorized } from './api';
import { Alert } from './components/ui/Alert';
import { Layout } from './components/Layout';
import { Certificates } from './pages/Certificates';
import { EditHost } from './pages/EditHost';
import { Host } from './pages/Host';
import { HostHistory } from './pages/HostHistory';
import { Overview } from './pages/Overview';
import { Server } from './pages/Server';
import { Hosts } from './pages/Hosts';
import { SignIn } from './pages/SignIn';
import { navigate, usePath } from './router';
import { SessionContext } from './api/session';
import './styles.css';

function Page({ path }: { path: string }) {
  if (path === '/hosts/new') return <EditHost name={null} />;
  if (path.startsWith('/hosts/') && path.endsWith('/history')) {
    return <HostHistory name={decodeURIComponent(path.slice('/hosts/'.length, -'/history'.length))} />;
  }
  if (path.startsWith('/hosts/') && path.endsWith('/edit')) {
    return <EditHost name={decodeURIComponent(path.slice('/hosts/'.length, -'/edit'.length))} />;
  }
  if (path.startsWith('/hosts/')) return <Host name={decodeURIComponent(path.slice('/hosts/'.length))} />;
  if (path === '/certificates') return <Certificates />;
  if (path === '/server') return <Server />;
  if (path === '/overview') return <Overview />;
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
    if (path === '/') navigate('/overview');
  }, [path]);

  if (error) {
    return (
      <div class="grid min-h-screen place-items-center p-6">
        <Alert class="max-w-md">{error}</Alert>
      </div>
    );
  }
  if (signedIn === undefined) return null;
  if (signedIn === null) return <SignIn onSignedIn={setSignedIn} />;
  return (
    <SessionContext.Provider value={signedIn}>
      <Layout path={path} session={signedIn} onSignOut={() => setSignedIn(null)}>
        <div key={path} class="motion-safe:animate-[page-in_160ms_ease-out]">
          <Page path={path} />
        </div>
      </Layout>
    </SessionContext.Provider>
  );
}

render(<App />, document.getElementById('app')!);
