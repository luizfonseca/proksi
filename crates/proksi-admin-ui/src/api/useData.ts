import { useCallback, useEffect, useState } from 'preact/hooks';
import { get, Unauthorized } from '.';

export type Data<T> =
  | { state: 'loading' }
  | { state: 'ready'; value: T }
  | { state: 'error'; message: string }
  | { state: 'unauthorized' };

/** `GET path`, again every `refresh` milliseconds while the page is open */
export function useData<T>(path: string, refresh = 5000): [Data<T>, () => void] {
  const [data, setData] = useState<Data<T>>({ state: 'loading' });
  const load = useCallback(() => {
    get<T>(path)
      .then((value) => setData({ state: 'ready', value }))
      .catch((err) =>
        setData(
          err instanceof Unauthorized
            ? { state: 'unauthorized' }
            : { state: 'error', message: String(err.message ?? err) },
        ),
      );
  }, [path]);
  useEffect(() => {
    load();
    const timer = setInterval(load, refresh);
    return () => clearInterval(timer);
  }, [load, refresh]);
  return [data, load];
}
