import { defineConfig } from 'vitest/config';
import wasm from 'vite-plugin-wasm';

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
  plugins: [wasm()],
  test: {
    root: '.',
    include: ['web/src/**/*.test.ts'],
    environment: 'node',
  },
});
