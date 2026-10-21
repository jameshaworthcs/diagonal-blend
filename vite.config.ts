import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import type { Plugin } from 'vite';
import { defineConfig } from 'vitest/config';
import wasm from 'vite-plugin-wasm';
import { VitePWA } from 'vite-plugin-pwa';

const mediapipeDir = resolve(import.meta.dirname, 'node_modules/@mediapipe/tasks-vision/wasm');
// The classic (non-module) builds: SIMD for every current browser, and the
// fallback that MediaPipe picks without SIMD.
const mediapipeFiles = readdirSync(mediapipeDir).filter((f) => /^vision_wasm(_nosimd)?_internal\.(js|wasm)$/.test(f));

/** Serve MediaPipe's WASM runtime from our own origin, in dev and in the build. */
function mediapipeAssets(): Plugin {
  return {
    name: 'diagonal-blend:mediapipe',
    configureServer(server) {
      server.middlewares.use('/mediapipe', (req: { url?: string }, res, next) => {
        const name = (req.url ?? '').replace(/^\//, '').split('?')[0];
        if (!mediapipeFiles.includes(name)) return next();
        res.setHeader('Content-Type', name.endsWith('.wasm') ? 'application/wasm' : 'text/javascript');
        res.end(readFileSync(resolve(mediapipeDir, name)));
      });
    },
    generateBundle() {
      for (const name of mediapipeFiles) {
        this.emitFile({ type: 'asset', fileName: `mediapipe/${name}`, source: readFileSync(resolve(mediapipeDir, name)) });
      }
    },
  };
}

/**
 * A Content-Security-Policy that makes the privacy promise mechanical: the
 * page may only talk to its own origin, so it cannot upload anything.
 * Build only, because Vite's dev server needs a websocket and inline scripts.
 */
function contentSecurityPolicy(): Plugin {
  const policy = (inline: string[]) => [
    "default-src 'self'",
    `script-src 'self' 'wasm-unsafe-eval' ${inline.join(' ')}`,
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' blob: data:",
    "media-src 'self' blob:",
    "connect-src 'self'",
    "worker-src 'self' blob:",
    "manifest-src 'self'",
    "object-src 'none'",
    "base-uri 'self'",
    "form-action 'self'",
  ].join('; ');
  // The one inline script (the theme, applied before first paint) is
  // allowed by its hash.
  const hashes = (html: string) =>
    [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map(
      (m) => `'sha256-${createHash('sha256').update(m[1]).digest('base64')}'`,
    );
  return {
    name: 'diagonal-blend:csp',
    apply: 'build',
    transformIndexHtml(html) {
      const csp = policy(hashes(html));
      return html.replace('<meta charset="utf-8" />', `<meta charset="utf-8" />\n    <meta http-equiv="Content-Security-Policy" content="${csp}" />`);
    },
  };
}

export default defineConfig({
  root: 'web',
  publicDir: 'public',
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    target: 'es2022',
    assetsInlineLimit: 0,
    sourcemap: false,
  },
  server: { port: 5173, strictPort: true },
  preview: { port: 4173, strictPort: true },
  plugins: [
    wasm(),
    mediapipeAssets(),
    contentSecurityPolicy(),
    VitePWA({
      strategies: 'injectManifest',
      srcDir: 'src',
      filename: 'sw.ts',
      injectRegister: false,
      registerType: 'autoUpdate',
      manifest: false,
      injectManifest: {
        globPatterns: ['**/*.{html,js,css,wasm,svg,png,webmanifest,task}', 'mediapipe/vision_wasm_internal.*'],
        globIgnores: ['**/screenshots/**', 'samples/**', 'mediapipe/vision_wasm_nosimd_internal.*'],
        maximumFileSizeToCacheInBytes: 16 * 1024 * 1024,
      },
      devOptions: { enabled: false },
    }),
  ],
  test: {
    root: '.',
    include: ['web/src/**/*.test.ts'],
    environment: 'node',
  },
});
