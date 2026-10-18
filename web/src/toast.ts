const region = () => document.getElementById('toasts')!;

/** A short message in the polite live region; it fades after a while. */
export function toast(message: string, kind: 'info' | 'warn' = 'info', ms = 4200): void {
  const el = document.createElement('div');
  el.className = `toast ${kind}`;
  el.textContent = message;
  region().append(el);
  setTimeout(() => el.remove(), ms);
}
