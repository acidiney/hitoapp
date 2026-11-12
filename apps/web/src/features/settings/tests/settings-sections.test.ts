import { expect, test } from "vitest";
import {
  resolveSection,
  sectionHref,
  tabSections,
  visibleSections,
} from "../model/sections";

test("tabs list the six sections and hide Accounts from members", () => {
  const labels = (admin: boolean) =>
    tabSections(admin).map((section) => section.label);
  expect(labels(true)).toEqual([
    "Profile",
    "Privacy & lock",
    "Music",
    "Playback",
    "Accounts",
    "About",
  ]);
  expect(labels(false)).toEqual([
    "Profile",
    "Privacy & lock",
    "Music",
    "Playback",
    "About",
  ]);
});

test("Plugins stays a section of its own without being a tab", () => {
  expect(visibleSections(false).map((section) => section.id)).toContain(
    "plugins",
  );
  expect(tabSections(true).map((section) => section.id)).not.toContain(
    "plugins",
  );
});

test("sections resolve from the section parameter with legacy aliases", () => {
  expect(resolveSection("plugins", false)).toBe("plugins");
  expect(resolveSection("privacy", false)).toBe("privacy");
  expect(resolveSection("library", false)).toBe("music");
  expect(resolveSection("metadata", false)).toBe("music");
  expect(resolveSection("downloads", false)).toBe("music");
  expect(resolveSection("updates", false)).toBe("about");
  expect(resolveSection(null, true)).toBe("profile");
  expect(resolveSection("nonsense", true)).toBe("profile");
});

test("administrator sections fall back for members", () => {
  expect(resolveSection("accounts", true)).toBe("accounts");
  expect(resolveSection("accounts", false)).toBe("profile");
});

test("section links are deep links on the settings route", () => {
  expect(sectionHref("plugins")).toBe("/settings?section=plugins");
  expect(sectionHref("about")).toBe("/settings?section=about");
});
