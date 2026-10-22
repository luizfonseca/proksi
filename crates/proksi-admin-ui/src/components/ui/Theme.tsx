import { useEffect, useState } from 'preact/hooks';
import { Moon, Sun } from './icons';

// daisyUI's themes in styles.css: `light`, and `black` for dark
type Theme = 'light' | 'black';

const KEY = 'proksi-theme';
/** Sent on `window` when the theme changes */
const CHANGED = 'proksi-theme';

/** The theme chosen in this browser, if any (otherwise the system's) */
function chosen(): Theme | null {
  try {
    const theme = localStorage.getItem(KEY);
    return theme === 'light' || theme === 'black' ? theme : null;
  } catch {
    return null;
  }
}

/** Applies the theme chosen in this browser, before the UI is drawn */
export function applyTheme() {
  const theme = chosen();
  if (theme) document.documentElement.dataset.theme = theme;
}

function shown(): Theme {
  const theme = document.documentElement.dataset.theme;
  if (theme === 'light' || theme === 'black') return theme;
  return matchMedia('(prefers-color-scheme: dark)').matches ? 'black' : 'light';
}

/** Switches between light and dark, remembered in this browser */
export function toggleTheme() {
  const next = shown() === 'black' ? 'light' : 'black';
  document.documentElement.dataset.theme = next;
  try {
    localStorage.setItem(KEY, next);
  } catch {
    // Not remembered (private window): it still applies to this page
  }
  dispatchEvent(new Event(CHANGED));
}

/** A button switching between light and dark */
export function ThemeToggle() {
  const [theme, setTheme] = useState(shown);
  useEffect(() => {
    const update = () => setTheme(shown());
    addEventListener(CHANGED, update);
    return () => removeEventListener(CHANGED, update);
  }, []);
  const dark = theme === 'black';
  return (
    <button
      type="button"
      class="btn btn-ghost btn-sm btn-square"
      onClick={toggleTheme}
      title={dark ? 'Light theme' : 'Dark theme'}
      aria-label={dark ? 'Use the light theme' : 'Use the dark theme'}
    >
      {dark ? <Sun /> : <Moon />}
    </button>
  );
}
