import { expect, test } from "vitest";
import { resolveSection, sectionHref, visibleGroups } from "../sections";

test("groups list the nine sections and hide Accounts from members", () => {
  const labels = (admin: boolean) =>
    visibleGroups(admin).map((group) => [
      group.label,
      group.sections.map((section) => section.label),
    ]);
  expect(labels(true)).toEqual([
    ["Your account", ["Profile", "Privacy & lock"]],
    [
      "Listening",
      ["Music & providers", "Plugins", "Playback", "Downloads & storage"],
    ],
    ["Server", ["Accounts", "Updates", "About"]],
  ]);
  expect(labels(false)[2][1]).toEqual(["Updates", "About"]);
});

test("sections resolve from the section parameter with legacy aliases", () => {
  expect(resolveSection("plugins", false)).toBe("plugins");
  expect(resolveSection("privacy", false)).toBe("privacy");
  expect(resolveSection("library", false)).toBe("music");
  expect(resolveSection("metadata", false)).toBe("music");
  expect(resolveSection(null, true)).toBe("music");
  expect(resolveSection("nonsense", true)).toBe("music");
});

test("administrator sections fall back for members", () => {
  expect(resolveSection("accounts", true)).toBe("accounts");
  expect(resolveSection("accounts", false)).toBe("music");
});

test("section links are deep links on the settings route", () => {
  expect(sectionHref("plugins")).toBe("/settings?section=plugins");
  expect(sectionHref("about")).toBe("/settings?section=about");
});
