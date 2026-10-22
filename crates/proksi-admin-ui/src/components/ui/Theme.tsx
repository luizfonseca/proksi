import { useState } from 'preact/hooks';
import { Moon, Sun } from './icons';

// daisyUI's themes in styles.css: `light`, and `black` for dark
type Theme = 'light' | 'black';

const KEY = 'proksi-theme';

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
  return chosen() ?? (matchMedia('(prefers-color-scheme: dark)').matches ? 'black' : 'light');
}

/** Switches between light and dark, remembered in this browser */
export function ThemeToggle() {
  const [theme, setTheme] = useState(shown);
  const toggle = () => {
    const next = theme === 'black' ? 'light' : 'black';
    document.documentElement.dataset.theme = next;
    try {
      localStorage.setItem(KEY, next);
    } catch {
      // Not remembered (private window): it still applies to this page
    }
    setTheme(next);
  };
  const dark = theme === 'black';
  return (
    <button type="button" class="btn btn-ghost btn-sm btn-square" onClick={toggle} title={dark ? 'Light theme' : 'Dark theme'} aria-label={dark ? 'Use the light theme' : 'Use the dark theme'}>
      {dark ? <Sun /> : <Moon />}
    </button>
  );
}
