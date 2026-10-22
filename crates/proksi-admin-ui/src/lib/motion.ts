// Motion that explains a change: numbers rolling to their new value, things flashing once as they
// change. None of it for those who asked their system for less motion.

import { useEffect, useRef, useState } from 'preact/hooks';

/** Whether this system asks for less motion */
export function reduced(): boolean {
  return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/** Fast at first, slowing down to its end: `progress` from 0 to 1 */
export function easeOut(progress: number): number {
  const clamped = Math.min(1, Math.max(0, progress));
  return 1 - (1 - clamped) ** 3;
}

/** `target`, reached over `duration` milliseconds from wherever it was (at once the first time, and without motion) */
export function useTween(target: number | null, duration = 300): number | null {
  const [shown, setShown] = useState(target);
  const current = useRef(target);
  useEffect(() => {
    const from = current.current;
    if (target === null || from === null || from === target || reduced()) {
      current.current = target;
      setShown(target);
      return;
    }
    const start = performance.now();
    let frame = requestAnimationFrame(function step(now) {
      const value = from + (target - from) * easeOut((now - start) / duration);
      current.current = value;
      setShown(value);
      if (now - start < duration) frame = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(frame);
  }, [target, duration]);
  return shown;
}

/** True for `duration` milliseconds after `value` changes (not when it's first seen) */
export function useChanged(value: unknown, duration = 1200): boolean {
  const previous = useRef(value);
  const [changed, setChanged] = useState(false);
  useEffect(() => {
    if (previous.current === value) return;
    previous.current = value;
    if (reduced()) return;
    setChanged(true);
    const timer = setTimeout(() => setChanged(false), duration);
    return () => clearTimeout(timer);
  }, [value, duration]);
  return changed;
}
