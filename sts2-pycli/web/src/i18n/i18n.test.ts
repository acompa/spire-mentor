import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { fill, getLang, langFromTag, resolveLang, setLang, t, tn, ZH } from "./index";

const SRC = join(__dirname, "..");

function sources(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) {
      if (name !== "__tests__" && name !== "i18n" && name !== "node_modules") {
        sources(p, out);
      }
    } else if (/\.(ts|tsx)$/.test(name) && !/\.test\.(ts|tsx)$/.test(name)) {
      out.push(p);
    }
  }
  return out;
}

/** The string literals passed to t() / tn() in `code` (template literals
 *  with `${}` cannot be checked and are skipped). */
export function literalKeys(code: string): string[] {
  const keys: string[] = [];
  const lit = String.raw`("(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|\`(?:[^\`\\$]|\\.)*\`)`;
  const unq = (s: string) => {
    const body = s.slice(1, -1);
    return body.replace(/\\(u\{[0-9a-fA-F]+\}|u[0-9a-fA-F]{4}|.)/g, (_m, e: string) => {
      if (e.startsWith("u{")) return String.fromCodePoint(parseInt(e.slice(2, -1), 16));
      if (e.startsWith("u") && e.length === 5) return String.fromCharCode(parseInt(e.slice(1), 16));
      return ({ n: "\n", t: "\t" } as Record<string, string>)[e] ?? e;
    });
  };
  for (const m of code.matchAll(new RegExp(String.raw`(?<![\w$.])t\(\s*` + lit, "g"))) {
    keys.push(unq(m[1]));
  }
  for (const m of code.matchAll(
    new RegExp(String.raw`(?<![\w$.])tn\(\s*[^,]+,\s*` + lit + String.raw`\s*,\s*` + lit, "g"),
  )) {
    keys.push(unq(m[1]), unq(m[2]));
  }
  return keys;
}

afterEach(() => setLang("en", false));

describe("i18n", () => {
  it("maps language tags", () => {
    expect(langFromTag("zh-CN")).toBe("zh");
    expect(langFromTag("zh-Hant-TW")).toBe("zh");
    expect(langFromTag("zh")).toBe("zh");
    expect(langFromTag("en-US")).toBe("en");
    expect(langFromTag("de-DE")).toBeNull();
    expect(langFromTag(undefined)).toBeNull();
  });

  it("resolves url > stored > browser > english", () => {
    expect(resolveLang("?lang=zh", "en", ["en-US"])).toEqual({ lang: "zh", fromUrl: true });
    expect(resolveLang("", "zh", ["en-US"]).lang).toBe("zh");
    expect(resolveLang("", null, ["de-DE", "zh-CN", "en"]).lang).toBe("zh");
    expect(resolveLang("", null, ["de-DE", "en-GB", "zh-CN"]).lang).toBe("en");
    expect(resolveLang("?lang=fr", null, ["fr-FR"]).lang).toBe("en");
  });

  it("translates and falls back to the english key", () => {
    expect(getLang()).toBe("en"); // jsdom's navigator is en-US
    expect(t("Cancel")).toBe("Cancel");
    setLang("zh", false);
    expect(t("Cancel")).toBe("取消");
    expect(t("no such string {x}", { x: 1 })).toBe("no such string 1");
  });

  it("fills placeholders and counts", () => {
    expect(fill("Floor {n} of {m}", { n: 3 })).toBe("Floor 3 of {m}");
    expect(tn(1, "{n} turn", "{n} turns")).toBe("1 turn");
    expect(tn(2, "{n} turn", "{n} turns")).toBe("2 turns");
  });

  it("extracts literal keys", () => {
    expect(literalKeys(`x = t("A b"); y = tn(n, 'one {n}', "many {n}"); z = st("no"); w = t(\`T\`)`)).toEqual([
      "A b",
      "T",
      "one {n}",
      "many {n}",
    ]);
  });

  it("no two zh tables translate one key differently", () => {
    const tables = import.meta.glob<{ default: Record<string, string> }>("./zh/*.ts", { eager: true });
    const seen = new Map<string, [string, string]>();
    const clashes: string[] = [];
    for (const [file, mod] of Object.entries(tables)) {
      for (const [k, v] of Object.entries(mod.default)) {
        const prev = seen.get(k);
        if (prev && prev[1] !== v) {
          clashes.push(`${JSON.stringify(k)}: ${prev[0]} ${JSON.stringify(prev[1])} vs ${file} ${JSON.stringify(v)}`);
        }
        if (!prev) seen.set(k, [file, v]);
      }
    }
    expect(clashes).toEqual([]);
  });

  it("every literal t() key has a Chinese entry with the same placeholders", () => {
    const missing: string[] = [];
    const badVars: string[] = [];
    const vars = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
    for (const file of sources(SRC)) {
      for (const key of literalKeys(readFileSync(file, "utf-8"))) {
        const zh = ZH[key];
        if (zh === undefined) {
          missing.push(`${file.slice(SRC.length + 1)}: ${JSON.stringify(key)}`);
        } else if (vars(zh) !== vars(key)) {
          badVars.push(`${JSON.stringify(key)} -> ${JSON.stringify(zh)}`);
        }
      }
    }
    expect(missing).toEqual([]);
    expect(badVars).toEqual([]);
  });
});
