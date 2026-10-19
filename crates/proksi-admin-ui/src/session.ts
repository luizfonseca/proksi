import { createContext } from 'preact';
import { useContext } from 'preact/hooks';
import type { Session } from './api';

export const SessionContext = createContext<Session>({ name: null, access: 'read' });

/** Whether the signed-in token can change hosts */
export function useCanWrite() {
  return useContext(SessionContext).access === 'write';
}
