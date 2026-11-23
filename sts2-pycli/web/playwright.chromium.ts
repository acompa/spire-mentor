import { existsSync } from "node:fs";

// A host that ships its own Chromium may not have the revision this
// Playwright version pins, and may forbid `playwright install` (the Claude
// Code web containers put one at /opt/pw-browsers/chromium; GH #1441).
// PLAYWRIGHT_CHROMIUM_PATH names one explicitly; both configs use the first
// of these that exists and fall back to Playwright's own browser.
export const chromiumPath = [process.env.PLAYWRIGHT_CHROMIUM_PATH, "/opt/pw-browsers/chromium"].find(
  (p): p is string => !!p && existsSync(p),
);

/** `use.launchOptions` for a project: empty unless a host Chromium was found. */
export const chromiumLaunch = chromiumPath ? { launchOptions: { executablePath: chromiumPath } } : {};
