import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import Menu from "./Menu";
import { languageItems } from "./languageItems";
import { getLang, LANG_KEY, setLang } from "../i18n";

afterEach(() => {
  setLang("en", false);
  localStorage.removeItem(LANG_KEY);
});

describe("language switch", () => {
  it("offers English and 中文, checks the current one, and reloads into the pick", () => {
    const reload = vi.fn();
    render(<Menu label="Tools" items={languageItems(reload)} />);
    fireEvent.click(screen.getByRole("button", { name: /Tools/ }));
    expect(screen.getByRole("menuitemcheckbox", { name: /English/ })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("menuitemcheckbox", { name: /中文/ })).toHaveAttribute("aria-checked", "false");

    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: /中文/ }));
    expect(getLang()).toBe("zh");
    expect(localStorage.getItem(LANG_KEY)).toBe("zh");
    expect(reload).toHaveBeenCalledOnce();
  });

  it("does not reload when the current language is picked again", () => {
    const reload = vi.fn();
    render(<Menu label="Tools" items={languageItems(reload)} />);
    fireEvent.click(screen.getByRole("button", { name: /Tools/ }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: /English/ }));
    expect(reload).not.toHaveBeenCalled();
  });
});
