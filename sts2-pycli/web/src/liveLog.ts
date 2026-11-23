/**
 * Follow mode for the workbench the Spire Mentor mod bundles.
 *
 * Follow (`?follow=1`, App.tsx) tails the run log the advisor is writing. The
 * Python workbench reads that file from disk; the browser build's run store
 * is IndexedDB, so this mirrors the file into it instead: the control plane
 * that serves the page answers `GET /log` (realgame_control.py), and a
 * changed log is written to the worker's run store with `POST /api/runs/put`
 * under its bare filename. From there the follow loop is the desktop one —
 * `/api/load` reopens it, and `/api/runs/stat` changes whenever a put did.
 *
 * Follow alone only ever sees the log being written while the page is open.
 * `importAdvisorLogs` fills in the rest: every log in the advisor's logs
 * directory (`GET /logs`) the store lacks, so Run history and the Replay list
 * hold the runs played with no workbench open, and earlier sessions'.
 */
import { bridgeFetch } from "./bridge";
import { apiFetch } from "./transport";

interface LogStat {
  name: string;
  size: number;
  mtime: number;
}

let mirrored: LogStat | null = null;

/** Mirror the advisor's current run log into the page's run store (only
 *  when it changed) and return its key there; null while there is none. */
export async function mirrorLiveLog(): Promise<string | null> {
  const r = await bridgeFetch("/log?stat=1");
  if (!r.ok) return null;
  const st = (await r.json()) as LogStat;
  if (mirrored && mirrored.name === st.name && mirrored.size === st.size && mirrored.mtime === st.mtime) {
    return st.name;
  }
  const full = await bridgeFetch("/log");
  if (!full.ok) return null;
  const log = (await full.json()) as LogStat & { text: string };
  // A log opened a moment ago holds no complete line yet; the run store
  // refuses empty text, and there is nothing to show anyway.
  if (!log.text.trim()) return null;
  const put = await apiFetch("/api/runs/put", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ path: log.name, text: log.text }),
  });
  if (!put.ok) return null;
  mirrored = { name: log.name, size: log.size, mtime: log.mtime };
  return log.name;
}

/** How many of the advisor's newest logs one import looks at. A dev logs
 *  directory can hold thousands; the page's store is the browser's. */
export const ADVISOR_LOG_IMPORT_LIMIT = 200;

let importing: Promise<number> | null = null;

/** Import the advisor's run logs (`GET /logs`, realgame_control.py) the run
 *  store is missing or holds an older copy of: Follow only mirrors the log
 *  being written while a page is open, so runs played with no workbench
 *  open, or finished after it closed, would never reach Run history or the
 *  Replay list. Resolves to how many logs it stored; concurrent calls share
 *  one pass. Never rejects. */
export function importAdvisorLogs(): Promise<number> {
  importing ??= importOnce()
    .catch(() => 0)
    .finally(() => {
      importing = null;
    });
  return importing;
}

async function importOnce(): Promise<number> {
  const r = await bridgeFetch("/logs");
  if (!r.ok) return 0;
  const { logs } = (await r.json()) as { logs?: LogStat[] };
  let stored = 0;
  for (const st of (logs ?? []).slice(0, ADVISOR_LOG_IMPORT_LIMIT)) {
    if (!st.size) continue;
    // The store keeps the text cut at its last complete line, so a finished
    // log's stored size is the file's; any difference means a newer copy.
    const have = await apiFetch(`/api/runs/stat?path=${encodeURIComponent(st.name)}`);
    if (have.ok && ((await have.json()) as { size: number }).size === st.size) continue;
    const full = await bridgeFetch(`/log?name=${encodeURIComponent(st.name)}`);
    if (!full.ok) continue;
    const log = (await full.json()) as LogStat & { text: string };
    if (!log.text.trim()) continue;
    const put = await apiFetch("/api/runs/put", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ path: st.name, text: log.text }),
    });
    if (put.ok) stored += 1;
  }
  return stored;
}

/** Tests: forget what was mirrored. */
export function resetLiveLogMirror(): void {
  mirrored = null;
  importing = null;
}
