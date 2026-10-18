// The Rust core, loaded after first paint so the page shows up at once.

export type Core = typeof import('./pkg/core.js');
export type Scene = import('./pkg/core.js').Scene;
export type Renderer = import('./pkg/core.js').Renderer;

let core: Promise<Core> | null = null;

export function loadCore(): Promise<Core> {
  core ??= import('./pkg/core.js');
  return core;
}
