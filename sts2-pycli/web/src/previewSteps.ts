// The board preview's scripted second steps (`boardpreview.tsx`): `?attack=`,
// `?debuff=`, `?buff=`, `?die=`, `?hit=`, `?summon=`.
//
// A number is a wall-clock delay in ms from the board's mount, which is what a
// human eyeballing the preview wants. `manual` arms the step without a timer:
// it runs when something calls `window.__preview.fire("<step>")`, which is what
// an e2e test wants. A timer starts with the page, not with the test's own
// steps, so a test that has to act BEFORE the step lands raced it on a loaded
// box (GH #1442).

export const PREVIEW_STEPS = ["attack", "debuff", "buff", "die", "hit", "summon"] as const;
export type PreviewStep = (typeof PREVIEW_STEPS)[number];

export type StepSchedule = { kind: "off" } | { kind: "timer"; ms: number } | { kind: "manual" };

export function isPreviewStep(name: string): name is PreviewStep {
  return (PREVIEW_STEPS as readonly string[]).includes(name);
}

/** A step's query value: absent, empty, `0` or not a number is off. */
export function parseStepParam(raw: string | null): StepSchedule {
  if (raw === "manual") return { kind: "manual" };
  const ms = Number(raw ?? 0);
  return Number.isFinite(ms) && ms > 0 ? { kind: "timer", ms } : { kind: "off" };
}

/** What `window.__preview.fire` returns: the step that ran, the board's step
 *  index once it has been rendered, and the manual steps still waiting. */
export interface FireResult {
  step: PreviewStep;
  idx: number;
  armed: PreviewStep[];
}

/** The page's `window.__preview`. */
export interface PreviewApi {
  /** Run an armed manual step now. Throws on an unknown step name, on one the
   *  page was not opened with as `manual`, and on one that already ran. */
  fire(step: string): FireResult;
  /** The manual steps armed and not yet fired. */
  armed(): PreviewStep[];
}

/** The manual steps of one page. Each runs at most once. */
export class ManualSteps {
  private readonly waiting = new Map<PreviewStep, () => number>();
  private readonly done = new Set<PreviewStep>();

  /** Arm `step`; `run` applies it and returns the board's step index after
   *  it. Returns the disarm (an effect cleanup). */
  arm(step: PreviewStep, run: () => number): () => void {
    this.waiting.set(step, run);
    return () => {
      if (this.waiting.get(step) === run) this.waiting.delete(step);
    };
  }

  armed(): PreviewStep[] {
    return PREVIEW_STEPS.filter((s) => this.waiting.has(s));
  }

  fire(step: string): FireResult {
    if (!isPreviewStep(step)) {
      throw new Error(`unknown preview step "${step}" (known: ${PREVIEW_STEPS.join(", ")})`);
    }
    const run = this.waiting.get(step);
    if (!run) {
      if (this.done.has(step)) throw new Error(`preview step "${step}" already fired`);
      const armed = this.armed();
      throw new Error(`preview step "${step}" is not armed: open the preview with ?${step}=manual`
        + ` (armed: ${armed.length ? armed.join(", ") : "none"})`);
    }
    this.waiting.delete(step);
    this.done.add(step);
    const idx = run();
    return { step, idx, armed: this.armed() };
  }
}
