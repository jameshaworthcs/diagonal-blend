import { readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import type { Plugin } from 'vite';
import { defineConfig } from 'vitest/config';
import wasm from 'vite-plugin-wasm';

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
  ],
  test: {
    root: '.',
    include: ['web/src/**/*.test.ts'],
    environment: 'node',
  },
});
