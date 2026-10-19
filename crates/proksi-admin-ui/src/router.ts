import { useEffect, useState } from 'preact/hooks';

// Paths are the UI's own pages: the admin listener answers them with index.html

export function navigate(path: string) {
  if (path === location.pathname) return;
  history.pushState(null, '', path);
  dispatchEvent(new PopStateEvent('popstate'));
}

export function usePath() {
  const [path, setPath] = useState(location.pathname);
  useEffect(() => {
    const update = () => setPath(location.pathname);
    addEventListener('popstate', update);
    return () => removeEventListener('popstate', update);
  }, []);
  return path;
}

/** A link that changes the page without reloading it */
export function onLink(event: MouseEvent) {
  const target = event.currentTarget as HTMLAnchorElement;
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.button !== 0) return;
  event.preventDefault();
  navigate(target.getAttribute('href') ?? '/');
}
