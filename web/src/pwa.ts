// Install prompt, offline readiness and the share-target inbox.

import { registerSW } from 'virtual:pwa-register';
import { toast } from './toast';

interface InstallPrompt extends Event {
  prompt(): Promise<void>;
  userChoice: Promise<{ outcome: 'accepted' | 'dismissed' }>;
}

export function setupPwa(): void {
  if ('serviceWorker' in navigator && import.meta.env.PROD) {
    registerSW({
      immediate: true,
      onOfflineReady: () => toast('Ready to work offline.'),
    });
  }
  const button = document.getElementById('install') as HTMLButtonElement;
  let deferred: InstallPrompt | null = null;
  window.addEventListener('beforeinstallprompt', (e) => {
    e.preventDefault();
    deferred = e as InstallPrompt;
    button.hidden = false;
  });
  button.addEventListener('click', async () => {
    if (!deferred) return;
    await deferred.prompt();
    await deferred.userChoice;
    deferred = null;
    button.hidden = true;
  });
  window.addEventListener('appinstalled', () => (button.hidden = true));
}

/** Photos shared into the app by the service worker, oldest first. */
export async function takeSharedPhotos(): Promise<File[]> {
  const params = new URLSearchParams(location.search);
  if (!params.has('shared')) return [];
  history.replaceState(null, '', location.pathname + location.hash);
  if (!('caches' in window)) return [];
  const inbox = await caches.open('share-inbox');
  const files: File[] = [];
  for (const req of await inbox.keys()) {
    const res = await inbox.match(req);
    if (!res) continue;
    const blob = await res.blob();
    const name = decodeURIComponent(res.headers.get('X-File-Name') ?? 'shared photo');
    files.push(new File([blob], name, { type: blob.type }));
    await inbox.delete(req);
  }
  return files;
}
