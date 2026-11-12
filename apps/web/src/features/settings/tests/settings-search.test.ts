import { expect, test } from "vitest";
import {
  isSettingsSearchShortcut,
  searchSettings,
  settingAnchor,
  settingHref,
} from "../model/settingsSearch";
import { setting } from "../model/settingsCatalog";
import { visibleSections } from "../model/sections";

const labels = (query: string, admin = true) =>
  searchSettings(query, admin).map((hit) => hit.label);

test("a blank query finds nothing", () => {
  expect(searchSettings("", true)).toEqual([]);
  expect(searchSettings("   ", true)).toEqual([]);
});

test("a section is found by its visible label and a setting by its card title", () => {
  expect(labels("plugins")[0]).toBe("Plugins");
  expect(labels("password")).toContain("Change password");
  expect(labels("screen lock")[0]).toBe("Screen lock");
});

test("results say which section holds each setting", () => {
  const hit = searchSettings("change password", true)[0];
  expect(hit).toMatchObject({
    kind: "setting",
    label: "Change password",
    sectionLabel: "Profile",
    place: "Profile",
    section: "profile",
    href: "/settings?section=profile#setting-change-password",
  });
});

test("a plugin query lists the Plugins section then its settings", () => {
  const hits = searchSettings("plugin", true);
  expect(hits[0]).toMatchObject({ kind: "section", label: "Plugins" });
  expect(
    hits.filter((hit) => hit.section === "plugins").length,
  ).toBeGreaterThan(2);
});

test("matching ignores case and accents and needs every word", () => {
  expect(labels("PASSWORD")).toContain("Change password");
  expect(labels("pássword")).toContain("Change password");
  expect(labels("change zebra")).toEqual([]);
  expect(labels("lock screen")).toContain("Screen lock");
});

test("a label match ranks above a description match", () => {
  const order = labels("password");
  expect(order.indexOf("Change password")).toBeLessThan(
    order.indexOf("Screen lock"),
  );
});

test("keywords find settings whose words are not in the title", () => {
  expect(labels("log out")).toContain("Sign out");
  expect(labels("pin")).toEqual(
    expect.arrayContaining(["PIN", "Lock when idle", "Lock now"]),
  );
  expect(searchSettings("pin", true)[0].place).toBe(
    "Privacy & lock · Screen lock",
  );
});

test("members never see administrator sections or settings", () => {
  expect(labels("accounts", false)).not.toContain("Accounts");
  expect(labels("accounts", false)).not.toContain("People");
  expect(labels("accounts", true)).toContain("People");
  expect(labels("corrections", false)).toEqual([]);
  expect(labels("service address", false)).not.toContain(
    "Services for this server",
  );
});

test("every result lands on a section the viewer can open", () => {
  for (const admin of [true, false]) {
    const open = new Set(visibleSections(admin).map((section) => section.id));
    for (const query of ["a", "e", "o", "s", "plugin", "password"])
      for (const hit of searchSettings(query, admin))
        expect(open.has(hit.section)).toBe(true);
  }
});

test("a setting links to its section with an anchor the card uses", () => {
  const entry = setting("screen-lock");
  expect(settingAnchor(entry.id)).toBe("setting-screen-lock");
  expect(settingHref(entry)).toBe(
    "/settings?section=privacy#setting-screen-lock",
  );
});

function press(init: KeyboardEventInit): KeyboardEvent {
  return new KeyboardEvent("keydown", { cancelable: true, ...init });
}

test("slash focuses the search unless a field is being typed in", () => {
  const input = document.createElement("input");
  expect(isSettingsSearchShortcut(press({ key: "/" }))).toBe(true);
  const typed = press({ key: "/" });
  Object.defineProperty(typed, "target", { value: input });
  expect(isSettingsSearchShortcut(typed)).toBe(false);
  expect(isSettingsSearchShortcut(press({ key: "/", ctrlKey: true }))).toBe(
    false,
  );
});

test("the browser find shortcut and the global K shortcut are left to their owners", () => {
  expect(isSettingsSearchShortcut(press({ key: "f", metaKey: true }))).toBe(
    false,
  );
  expect(isSettingsSearchShortcut(press({ key: "f", ctrlKey: true }))).toBe(
    false,
  );
  expect(isSettingsSearchShortcut(press({ key: "k", metaKey: true }))).toBe(
    false,
  );
});
