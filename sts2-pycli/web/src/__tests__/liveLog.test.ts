import { beforeEach, describe, expect, it, vi } from "vitest";

const bridgeFetch = vi.fn();
const apiFetch = vi.fn();
vi.mock("../bridge", () => ({ bridgeFetch: (...a: unknown[]) => bridgeFetch(...a) }));
vi.mock("../transport", () => ({ apiFetch: (...a: unknown[]) => apiFetch(...a) }));

import { ADVISOR_LOG_IMPORT_LIMIT, importAdvisorLogs, mirrorLiveLog, resetLiveLogMirror } from "../liveLog";

const json = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status }));

describe("liveLog.ts — Follow for the bundled workbench", () => {
  let log = { name: "20261016_IRONCLAD_S1.jsonl", size: 12, mtime: 100, text: '{"step":0}\n' };
  beforeEach(() => {
    resetLiveLogMirror();
    bridgeFetch.mockReset();
    apiFetch.mockReset();
    log = { name: "20261016_IRONCLAD_S1.jsonl", size: 12, mtime: 100, text: '{"step":0}\n' };
    bridgeFetch.mockImplementation((path: string) =>
      path === "/log?stat=1" ? json({ name: log.name, size: log.size, mtime: log.mtime }) : json(log),
    );
    apiFetch.mockImplementation(() => json({ path: log.name, size: log.size }));
  });

  it("puts the advisor's log into the run store, then only again when it changed", async () => {
    expect(await mirrorLiveLog()).toBe(log.name);
    expect(apiFetch).toHaveBeenCalledTimes(1);
    const [route, init] = apiFetch.mock.calls[0] as [string, RequestInit];
    expect(route).toBe("/api/runs/put");
    expect(JSON.parse(init.body as string)).toEqual({ path: log.name, text: log.text });

    expect(await mirrorLiveLog()).toBe(log.name); // unchanged: a stat, no transfer
    expect(apiFetch).toHaveBeenCalledTimes(1);
    expect(bridgeFetch).toHaveBeenCalledTimes(3);

    log = { ...log, size: 24, mtime: 101, text: '{"step":0}\n{"step":1}\n' };
    expect(await mirrorLiveLog()).toBe(log.name);
    expect(apiFetch).toHaveBeenCalledTimes(2);
  });

  it("is null while the advisor has no log, or only a torn first line", async () => {
    bridgeFetch.mockImplementationOnce(() => json({ error: "no_log" }, 404));
    expect(await mirrorLiveLog()).toBeNull();
    log = { ...log, text: "" };
    expect(await mirrorLiveLog()).toBeNull();
    expect(apiFetch).not.toHaveBeenCalled();
  });
});

describe("liveLog.ts — importing the advisor's earlier logs", () => {
  const files: Record<string, string> = {
    "old_A.jsonl": '{"a":1}\n',
    "old_B.jsonl": '{"b":1}\n',
    "changed.jsonl": '{"c":1}\n{"c":2}\n',
  };
  const size = (s: string) => new TextEncoder().encode(s).length;
  let store: Record<string, number>;
  beforeEach(() => {
    resetLiveLogMirror();
    bridgeFetch.mockReset();
    apiFetch.mockReset();
    store = { "old_B.jsonl": size(files["old_B.jsonl"]), "changed.jsonl": 8 };
    bridgeFetch.mockImplementation((path: string) => {
      if (path === "/logs") {
        return json({ logs: [...Object.entries(files).map(([name, t]) => ({ name, size: size(t), mtime: 1 })),
          { name: "empty.jsonl", size: 0, mtime: 1 }] });
      }
      const name = decodeURIComponent(path.split("name=")[1] ?? "");
      return json({ name, size: size(files[name]), mtime: 1, text: files[name] });
    });
    apiFetch.mockImplementation((route: string, init?: RequestInit) => {
      if (route.startsWith("/api/runs/stat")) {
        const name = decodeURIComponent(route.split("path=")[1]);
        return name in store ? json({ size: store[name], mtime: 1 }) : json({ detail: "nope" }, 404);
      }
      const body = JSON.parse(init!.body as string) as { path: string; text: string };
      store[body.path] = size(body.text);
      return json({ path: body.path });
    });
  });

  it("stores the logs the run store lacks or holds an older copy of, and only those", async () => {
    expect(await importAdvisorLogs()).toBe(2);
    const puts = apiFetch.mock.calls.filter(([r]) => r === "/api/runs/put")
      .map(([, init]) => (JSON.parse((init as RequestInit).body as string) as { path: string }).path);
    expect(puts).toEqual(["old_A.jsonl", "changed.jsonl"]);
    expect(bridgeFetch).not.toHaveBeenCalledWith("/log?name=empty.jsonl");
    expect(await importAdvisorLogs()).toBe(0); // all in sync now
  });

  it("shares one pass between concurrent callers and never rejects", async () => {
    const [a, b] = await Promise.all([importAdvisorLogs(), importAdvisorLogs()]);
    expect(a + b).toBe(4); // the same pass, reported to both
    expect(bridgeFetch.mock.calls.filter(([p]) => p === "/logs")).toHaveLength(1);
    bridgeFetch.mockImplementation(() => Promise.reject(new Error("advisor gone")));
    expect(await importAdvisorLogs()).toBe(0);
  });

  it("looks at the newest logs only", async () => {
    const many = Array.from({ length: ADVISOR_LOG_IMPORT_LIMIT + 5 }, (_, i) => ({ name: `r${i}.jsonl`, size: 8, mtime: 1 }));
    bridgeFetch.mockImplementation((path: string) =>
      path === "/logs" ? json({ logs: many }) : json({ name: "x", size: 8, mtime: 1, text: '{"a":1}\n' }));
    expect(await importAdvisorLogs()).toBe(ADVISOR_LOG_IMPORT_LIMIT);
  });
});
