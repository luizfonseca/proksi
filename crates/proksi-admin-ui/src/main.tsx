import { render } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { get, Unauthorized } from './api';
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
  const [signedIn, setSignedIn] = useState<boolean | null>(null);

  useEffect(() => {
    get('/status')
      .then(() => setSignedIn(true))
      .catch((err) => setSignedIn(!(err instanceof Unauthorized)));
  }, []);
  useEffect(() => {
    if (path === '/') navigate('/hosts');
  }, [path]);

  if (signedIn === null) return null;
  if (!signedIn) return <SignIn onSignedIn={() => setSignedIn(true)} />;
  return (
    <Layout path={path}>
      <Page path={path} />
    </Layout>
  );
}

render(<App />, document.getElementById('app')!);
