import { useEffect, useRef, useState } from 'preact/hooks';
import { get, type Host } from '../../api';
import { useCanWrite } from '../../api/session';
import { Search } from '../../components/ui/icons';
import { toggleTheme } from '../../components/ui/Theme';
import { navigate } from '../../router';
import { ACTIONS, type Command, hostCommands, PAGES, search } from './commands';

/** Sent on `window` to open the palette */
const OPEN = 'proksi-palette';

const MAC = /Mac|iPhone|iPad/.test(navigator.platform);

/** A key, outlined and quiet */
const KEY = 'kbd kbd-xs border-base-300 bg-transparent text-base-content/60 shadow-none';

/** The shortcut opening the palette, as this system writes it */
export const SHORTCUT = MAC ? '⌘K' : 'Ctrl K';

/** A button in a bar, opening the palette */
export function PaletteButton() {
  return (
    <button
      type="button"
      class="btn btn-sm justify-start gap-2 border-base-300 bg-base-100 font-normal text-base-content/60 shadow-none hover:bg-base-200 md:w-52"
      onClick={() => dispatchEvent(new Event(OPEN))}
    >
      <Search />
      <span class="max-md:hidden">Search</span>
      <kbd class={`${KEY} ml-auto max-md:hidden`}>{SHORTCUT}</kbd>
    </button>
  );
}

/** Pages, hosts and actions, found by typing: opened with ⌘K (Ctrl+K) anywhere */
export function Palette({ onSignOut }: { onSignOut: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const [hosts, setHosts] = useState<Host[]>([]);
  const canWrite = useCanWrite();

  useEffect(() => {
    const show = () => setOpen(true);
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setOpen((was) => !was);
      }
    };
    addEventListener(OPEN, show);
    addEventListener('keydown', onKey);
    return () => {
      removeEventListener(OPEN, show);
      removeEventListener('keydown', onKey);
    };
  }, []);

  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (open && !element.open) {
      setQuery('');
      setActive(0);
      element.showModal();
      // Hosts as they are now, each time it opens
      get<Host[]>('/hosts').then(setHosts).catch(() => setHosts([]));
    } else if (!open && element.open) {
      element.close();
    }
  }, [open]);

  // The highlighted one stays in view as the arrows move it
  useEffect(() => {
    dialog.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  const found = search([...PAGES, ...hostCommands(hosts, canWrite), ...ACTIONS], query);
  const run = (command: Command | undefined) => {
    if (!command) return;
    setOpen(false);
    if ('href' in command.run) navigate(command.run.href);
    else if (command.run.action === 'theme') toggleTheme();
    else onSignOut();
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const step = event.key === 'ArrowDown' ? 1 : -1;
      setActive((at) => (found.length ? (at + step + found.length) % found.length : 0));
    } else if (event.key === 'Enter') {
      event.preventDefault();
      run(found[active]);
    }
  };

  return (
    <dialog ref={dialog} class="modal items-start pt-[12vh]" onClose={() => setOpen(false)} aria-label="Search pages, hosts and actions">
      <div class="modal-box max-w-xl p-0">
        <label class="flex items-center gap-3 border-b border-base-300 px-4">
          <Search class="size-4 text-base-content/45" />
          <input
            class="h-12 grow bg-transparent outline-none"
            placeholder="Search pages, hosts and actions"
            value={query}
            autoFocus
            onInput={(event) => {
              setQuery(event.currentTarget.value);
              setActive(0);
            }}
            onKeyDown={onKeyDown}
          />
          <kbd class={KEY}>esc</kbd>
        </label>
        {found.length === 0 ? (
          <p class="px-4 py-8 text-center text-sm text-base-content/60">Nothing matches “{query.trim()}”</p>
        ) : (
          <ul class="max-h-80 overflow-y-auto p-2" role="listbox">
            {found.map((command, index) => (
              <li role="option" aria-selected={index === active}>
                <button
                  type="button"
                  class={`flex w-full items-center justify-between gap-3 rounded-field px-3 py-2 text-left text-sm ${index === active ? 'bg-base-200' : ''}`}
                  onMouseMove={() => setActive(index)}
                  onClick={() => run(command)}
                >
                  <span class="truncate">{command.title}</span>
                  <span class="shrink-0 text-xs text-base-content/45">{command.group}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
      <form method="dialog" class="modal-backdrop">
        <button aria-label="Close">close</button>
      </form>
    </dialog>
  );
}
