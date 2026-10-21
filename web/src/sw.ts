/// <reference lib="webworker" />
// The service worker: everything precached for offline use, plus the Web
// Share Target that lets a phone share two photos straight into the app.

import { cleanupOutdatedCaches, createHandlerBoundToURL, precacheAndRoute } from 'workbox-precaching';
import { NavigationRoute, registerRoute } from 'workbox-routing';
import { CacheFirst } from 'workbox-strategies';

declare const self: ServiceWorkerGlobalScope;

export const INBOX = 'share-inbox';

precacheAndRoute(self.__WB_MANIFEST);
cleanupOutdatedCaches();
registerRoute(new NavigationRoute(createHandlerBoundToURL('/index.html'), { denylist: [/^\/share-target/] }));
// The sample photos and MediaPipe's non-SIMD fallback: cached on first use.
registerRoute(({ url }) => url.pathname.startsWith('/samples/') || url.pathname.startsWith('/mediapipe/'), new CacheFirst({ cacheName: 'on-demand' }));

self.addEventListener('install', () => void self.skipWaiting());
self.addEventListener('activate', (event) => event.waitUntil(self.clients.claim()));

// Shared photos arrive as a multipart POST. Park them in a cache that only
// this origin can read, then send the page to pick them up.
self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url);
  if (event.request.method !== 'POST' || url.pathname !== '/share-target') return;
  event.respondWith(
    (async () => {
      const form = await event.request.formData();
      const files = form.getAll('photos').filter((f): f is File => f instanceof File && f.size > 0);
      const inbox = await caches.open(INBOX);
      for (const key of await inbox.keys()) await inbox.delete(key);
      await Promise.all(
        files.slice(0, 2).map((f, i) =>
          inbox.put(
            `/share-inbox/${i}`,
            new Response(f, { headers: { 'Content-Type': f.type || 'application/octet-stream', 'X-File-Name': encodeURIComponent(f.name) } }),
          ),
        ),
      );
      return Response.redirect(`/?shared=${files.length}`, 303);
    })(),
  );
});
