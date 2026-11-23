import { describe, expect, it, vi } from "vitest";
import { ManualSteps, parseStepParam } from "./previewSteps";

describe("parseStepParam", () => {
  it("reads a delay in ms as a timer", () => {
    expect(parseStepParam("6000")).toEqual({ kind: "timer", ms: 6000 });
  });

  it("reads `manual` as a step a test fires", () => {
    expect(parseStepParam("manual")).toEqual({ kind: "manual" });
  });

  it("is off when absent, zero or not a number", () => {
    for (const raw of [null, "", "0", "soon", "-5"]) expect(parseStepParam(raw)).toEqual({ kind: "off" });
  });
});

describe("ManualSteps", () => {
  it("runs an armed step once, when fired", () => {
    const steps = new ManualSteps();
    const run = vi.fn(() => 1);
    steps.arm("buff", run);
    expect(run).not.toHaveBeenCalled();
    expect(steps.fire("buff")).toEqual({ step: "buff", idx: 1, armed: [] });
    expect(run).toHaveBeenCalledTimes(1);
    expect(() => steps.fire("buff")).toThrow(/already fired/);
    expect(run).toHaveBeenCalledTimes(1);
  });

  it("keeps the other armed steps waiting", () => {
    const steps = new ManualSteps();
    steps.arm("summon", () => 1);
    steps.arm("buff", () => 2);
    expect(steps.armed()).toEqual(["buff", "summon"]);
    expect(steps.fire("summon").armed).toEqual(["buff"]);
  });

  it("throws on an unknown step", () => {
    expect(() => new ManualSteps().fire("explode")).toThrow(/unknown preview step "explode"/);
  });

  it("throws on a step the page did not arm", () => {
    const steps = new ManualSteps();
    steps.arm("die", () => 1);
    expect(() => steps.fire("hit")).toThrow(/"hit" is not armed: open the preview with \?hit=manual \(armed: die\)/);
  });

  it("disarms on cleanup, and a stale cleanup leaves a re-arm alone", () => {
    // StrictMode mounts, cleans up and re-runs an effect.
    const steps = new ManualSteps();
    const first = steps.arm("hit", () => 1);
    first();
    expect(steps.armed()).toEqual([]);
    steps.arm("hit", () => 2);
    first();
    expect(steps.fire("hit").idx).toBe(2);
  });
});
