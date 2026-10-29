import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import {
  focusShellSearch,
  handleSearchShortcut,
  isApplePlatform,
  openSearch,
  shortcutLabel,
} from "../searchShortcut";

vi.mock("@inertiajs/react", () => ({ router: { get: vi.fn() } }));

let field: HTMLInputElement | null = null;

function addField() {
  field = document.createElement("input");
  field.id = "shell-search";
  document.body.append(field);
  return field;
}

function press(init: KeyboardEventInit, mac: boolean, target?: Element) {
  const event = new KeyboardEvent("keydown", {
    key: "k",
    bubbles: true,
    cancelable: true,
    ...init,
  });
  (target ?? document.body).dispatchEvent(event);
  handleSearchShortcut(event, mac);
  return event;
}

beforeEach(() => {
  field = null;
});
afterEach(() => {
  field?.remove();
  document.body.innerHTML = "";
});

test("macOS answers to Command+K only and other systems to Control+K only", () => {
  const input = addField();
  expect(press({ metaKey: true }, true).defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(input);
  input.blur();
  expect(press({ ctrlKey: true }, true).defaultPrevented).toBe(false);
  expect(document.activeElement).not.toBe(input);
  expect(press({ ctrlKey: true }, false).defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(input);
  input.blur();
  expect(press({ metaKey: true }, false).defaultPrevented).toBe(false);
});

test("extra modifiers, composition and handled events are ignored", () => {
  addField();
  expect(press({ metaKey: true, shiftKey: true }, true).defaultPrevented).toBe(
    false,
  );
  expect(press({ metaKey: true, altKey: true }, true).defaultPrevented).toBe(
    false,
  );
  expect(
    press({ metaKey: true, isComposing: true }, true).defaultPrevented,
  ).toBe(false);
  const handled = new KeyboardEvent("keydown", {
    key: "k",
    metaKey: true,
    cancelable: true,
  });
  handled.preventDefault();
  handleSearchShortcut(handled, true);
  expect(document.activeElement).not.toBe(field);
  expect(press({ metaKey: true, key: "j" }, true).defaultPrevented).toBe(false);
});

test("Control+K is left to a text field on a Mac", () => {
  const input = addField();
  const area = document.createElement("textarea");
  document.body.append(area);
  area.focus();
  expect(press({ ctrlKey: true }, true, area).defaultPrevented).toBe(false);
  expect(document.activeElement).toBe(area);
  expect(input.matches(":focus")).toBe(false);
});

test("nothing happens while a dialog is open", () => {
  const input = addField();
  const dialog = document.createElement("dialog");
  dialog.setAttribute("open", "");
  document.body.append(dialog);
  expect(press({ metaKey: true }, true).defaultPrevented).toBe(false);
  expect(document.activeElement).not.toBe(input);
  expect(router.get).not.toHaveBeenCalled();
});

test("without a search field it opens the library and focuses the field on arrival", async () => {
  openSearch();
  expect(router.get).toHaveBeenCalledWith(
    "/library",
    {},
    expect.objectContaining({ onSuccess: expect.any(Function) }),
  );
  const options = vi.mocked(router.get).mock.calls[0][2] as {
    onSuccess: () => void;
  };
  options.onSuccess();
  const input = addField();
  await vi.waitFor(() => expect(document.activeElement).toBe(input));
});

test("focusShellSearch reports whether a field was there", () => {
  expect(focusShellSearch()).toBe(false);
  addField();
  expect(focusShellSearch()).toBe(true);
});

test("the hint matches the platform", () => {
  expect(shortcutLabel(true)).toEqual({ text: "⌘K", keys: "Meta+K" });
  expect(shortcutLabel(false)).toEqual({ text: "Ctrl K", keys: "Control+K" });
  expect(isApplePlatform("MacIntel")).toBe(true);
  expect(isApplePlatform("Win32")).toBe(false);
});
