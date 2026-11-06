import { afterEach, describe, expect, it } from "vitest";
import { applyTheme, isTheme, loadTheme, resolveTheme, saveTheme } from "./theme";

afterEach(() => {
  window.localStorage.clear();
  delete document.documentElement.dataset.theme;
});

describe("theme", () => {
  it("resolves system theme from the OS preference", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("validates theme values", () => {
    expect(isTheme("dark")).toBe(true);
    expect(isTheme("sepia")).toBe(false);
    expect(isTheme(null)).toBe(false);
  });

  it("defaults to system and ignores invalid stored values", () => {
    expect(loadTheme()).toBe("system");
    window.localStorage.setItem("notenest.theme", "garbage");
    expect(loadTheme()).toBe("system");
  });

  it("round-trips a saved theme", () => {
    saveTheme("dark");
    expect(loadTheme()).toBe("dark");
  });

  it("applies the resolved theme to the document", () => {
    applyTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});
