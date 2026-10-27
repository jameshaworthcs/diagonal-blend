import { defineConfig, devices } from '@playwright/test';

// Software rendering everywhere, so snapshots do not depend on the GPU:
// SwiftShader's Vulkan for WebGPU, and its GL for WebGL2.
const webgpu = [
  '--enable-unsafe-webgpu',
  '--enable-features=Vulkan',
  '--use-vulkan=swiftshader',
  '--use-webgpu-adapter=swiftshader',
  '--use-angle=swiftshader',
];
const webgl2 = ['--disable-features=WebGPU', '--use-angle=swiftshader'];

// Headless Firefox has no GL context on Linux, so it runs headed. Without a
// display (run `make test`, which starts Xvfb when it can) it is skipped.
const firefox = process.env.DISPLAY
  ? [
      {
        name: 'firefox-webgl2',
        use: { ...devices['Desktop Firefox'], viewport: { width: 1280, height: 860 }, headless: false },
        grepInvert: /@mobile-only|@p3-only/,
      },
    ]
  : [];

// BASE_URL points the suite at a deployed site instead of a local preview.
const baseURL = process.env.BASE_URL ?? 'http://localhost:4173';

export default defineConfig({
  testDir: 'tests/e2e',
  snapshotPathTemplate: '{testDir}/__snapshots__/{projectName}/{arg}{ext}',
  timeout: 120_000,
  expect: {
    timeout: 20_000,
    toHaveScreenshot: { maxDiffPixelRatio: 0.004, threshold: 0.15, animations: 'disabled' },
    toMatchSnapshot: { maxDiffPixelRatio: 0.004, threshold: 0.15 },
  },
  fullyParallel: true,
  workers: process.env.CI ? 2 : 4,
  retries: process.env.CI ? 1 : 0,
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL,
    viewport: { width: 1280, height: 860 },
    deviceScaleFactor: 1,
    colorScheme: 'light',
    acceptDownloads: true,
    trace: 'retain-on-failure',
  },
  projects: [
    {
      name: 'chromium-webgpu',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 860 }, launchOptions: { args: webgpu } },
      grepInvert: /@mobile-only|@p3-only/,
    },
    {
      name: 'chromium-webgl2',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 860 }, launchOptions: { args: webgl2 } },
      grepInvert: /@mobile-only|@p3-only/,
    },
    ...firefox,
    { name: 'mobile-chromium', use: { ...devices['Pixel 7'], launchOptions: { args: webgpu } }, grep: /@mobile/ },
    // A Display P3 screen, for the wide-gamut preview on both backends.
    ...(['webgpu', 'webgl2'] as const).map((backend) => ({
      name: `chromium-p3-${backend}`,
      use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 860 }, launchOptions: { args: [...(backend === 'webgpu' ? webgpu : webgl2), '--force-color-profile=display-p3-d65'] } },
      grep: /@p3-only/,
    })),
  ],
  webServer: process.env.BASE_URL
    ? undefined
    : {
        command: 'npm run preview',
        url: baseURL,
        reuseExistingServer: !process.env.CI,
        timeout: 30_000,
      },
});
