/** The seed checker's engine routes (answered by the page's engine worker). */
import { apiFetch } from "../transport";
import type { Seq } from "./protocol";
import type { Summary } from "./verdict";

export interface Preview {
  seed: string;
  length_ok: boolean;
  neow: string[];
  act1: string;
  bosses: string[][];
  character?: string;
  ascension: number;
}

async function post<T>(route: string, body: unknown): Promise<T> {
  const r = await apiFetch(route, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const j = (await r.json().catch(() => null)) as (T & { detail?: string }) | null;
  if (!r.ok) throw new Error(j?.detail ?? `${route}: ${r.status}`);
  return j as T;
}

/** Every route takes the run's `character` last (the engine's default:
 *  Ironclad). */
export interface SeedCheckApi {
  preview(seed: string, ascension: number, character?: string): Promise<Preview>;
  summary(seed: string, ascension: number, classes: number[], character?: string): Promise<Summary>;
  saveLog(
    seed: string, ascension: number, classes: number[], seq: Seq, k: number, character?: string,
  ): Promise<{ path: string }>;
}

export const seedCheckApi: SeedCheckApi = {
  preview: (seed, ascension, character) => post("/api/seedcheck/preview", { seed, ascension, character }),
  summary: (seed, ascension, classes, character) =>
    post("/api/seedcheck/summary", { seed, ascension, classes, character }),
  saveLog: (seed, ascension, classes, seq, k, character) =>
    post("/api/seedcheck/log", { seed, ascension, classes, seq, k, character }),
};
