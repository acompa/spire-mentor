import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";
import { chromiumLaunch } from "./playwright.chromium";

// End-to-end tests drive the real workbench: the Vite dev server (proxying
// /api to the FastAPI backend) + the in-process Rust engine. They are slow and
// stateful (one shared backend), so run serially with generous timeouts.
//
// Launch both servers via `webServer`. The backend reuses the already-built
// `sts2_rs` wheel in `.venv` (no maturin rebuild), reload off for stability.
// `STS2_E2E_BASE` points the run at a vite on its own port (`STS2_VITE_PORT=…
// scripts/dev-e2e.sh`), so a run never starts or reuses another worktree's
// :5173. The vite `webServer` follows it: it waits on that URL and, when
// nothing answers there, starts vite on that URL's port (the protocol's
// default, 80 / 443, when the URL names none). `STS2_E2E_BACKEND_PORT` does
// the same for the backend (default 8077), and the vite it starts proxies to
// that port; `just e2e-phone` sets both to free ports.
// A host Chromium (PLAYWRIGHT_CHROMIUM_PATH, /opt/pw-browsers/chromium) is
// used when present: playwright.chromium.ts.
const base = process.env.STS2_E2E_BASE ?? "http://localhost:5173";
const baseUrl = new URL(base);
const vitePort = baseUrl.port || (baseUrl.protocol === "https:" ? "443" : "80");
const backendPort = process.env.STS2_E2E_BACKEND_PORT ?? "8077";
const backend = `http://127.0.0.1:${backendPort}`;
// By absolute path, and marked with `-X sts2_e2e_backend` (CPython keeps
// unknown -X options in sys._xoptions and otherwise ignores them), so a
// backend that outlives an interrupted run names its checkout in the process
// list and `scripts/e2e-phone.sh` can find and stop it.
const pycli = fileURLToPath(new URL("..", import.meta.url));
const python = process.platform === "win32" ? join(pycli, ".venv", "Scripts", "python.exe") : join(pycli, ".venv", "bin", "python");
const backendMarker = "sts2_e2e_backend";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  workers: 1,
  timeout: 120_000,
  expect: { timeout: 15_000 },
  reporter: [["list"]],
  use: {
    baseURL: base,
    headless: true,
    trace: "on-first-retry",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], ...chromiumLaunch } }],
  webServer: [
    {
      command: `"${python}" -X ${backendMarker} -m workbench`,
      cwd: "..",
      url: `${backend}/api/health`,
      timeout: 180_000,
      reuseExistingServer: true,
      env: { PYTHONPATH: "python", STS2_RELOAD: "0", STS2_HOST: "127.0.0.1", STS2_PORT: backendPort },
    },
    {
      command: `npm run dev -- --port ${vitePort} --strictPort`,
      url: base,
      timeout: 60_000,
      reuseExistingServer: true,
      env: { STS2_BACKEND: backend },
    },
  ],
});
