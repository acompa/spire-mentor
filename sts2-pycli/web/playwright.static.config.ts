import { defineConfig, devices } from "@playwright/test";
import { chromiumLaunch } from "./playwright.chromium";

// A host Chromium (PLAYWRIGHT_CHROMIUM_PATH, /opt/pw-browsers/chromium) is
// found by playwright.chromium.ts; re-exported for e2e-static/bridge.spec.ts.
export { chromiumPath } from "./playwright.chromium";

// End-to-end tests for the BROWSER-ONLY workbench: no Python backend at all.
// The SPA is built in static mode (`npm run build:static`, which needs the
// wasm bundle from `just wasm-build`) and served by `vite preview`; every
// `/api/*` call is answered by the Rust engine running in the Web Worker.
//
// Run: `npx playwright test -c playwright.static.config.ts` (or `just
// e2e-static` from sts2-pycli/). The specs live in e2e-static/ so the default
// config (which boots the Python backend) never picks them up.
export default defineConfig({
  testDir: "./e2e-static",
  fullyParallel: false,
  workers: 1,
  timeout: 180_000,
  expect: { timeout: 20_000 },
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:4173",
    headless: true,
    trace: "on-first-retry",
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        ...chromiumLaunch,
      },
    },
  ],
  webServer: {
    command: "npm run preview:static -- --port 4173 --strictPort",
    url: "http://localhost:4173",
    timeout: 60_000,
    reuseExistingServer: true,
  },
});
