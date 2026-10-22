// Icons (16px unless given a size, stroke), drawn for the UI
const props = {
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  'stroke-width': 1.8,
  'stroke-linecap': 'round' as const,
  'stroke-linejoin': 'round' as const,
};

type Icon = { class?: string };

export const Globe = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></svg>
);
export const Shield = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><path d="M12 3l8 3v6c0 4.5-3.4 8.3-8 9-4.6-.7-8-4.5-8-9V6z" /><path d="M9 12l2 2 4-4" /></svg>
);
export const Pulse = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><path d="M3 12h4l3-8 4 16 3-8h4" /></svg>
);
export const Lock = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V8a4 4 0 0 1 8 0v3" /></svg>
);
export const Unlock = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V8a4 4 0 0 1 7.5-2" /></svg>
);
export const Search = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><circle cx="11" cy="11" r="7" /><path d="M20 20l-3.5-3.5" /></svg>
);
export const Server = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><rect x="4" y="4" width="16" height="7" rx="2" /><rect x="4" y="13" width="16" height="7" rx="2" /><path d="M8 7.5h.01M8 16.5h.01" /></svg>
);
export const Plus = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><path d="M12 5v14M5 12h14" /></svg>
);
export const Pencil = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><path d="M4 20h4L19 9l-4-4L4 16z" /><path d="M13.5 6.5l4 4" /></svg>
);
export const Clock = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></svg>
);
export const SignOut = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><path d="M14 4h4a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-4" /><path d="M10 16l-4-4 4-4M6 12h10" /></svg>
);
export const Bell = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><path d="M6 16V11a6 6 0 0 1 12 0v5l2 2H4z" /><path d="M10 21h4" /></svg>
);
export const Book = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><path d="M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2z" /><path d="M4 19V5M8 7h7" /></svg>
);
export const External = ({ class: size = 'size-4' }: Icon) => (
  <svg {...props} class={`shrink-0 ${size}`}><path d="M14 4h6v6M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
);
