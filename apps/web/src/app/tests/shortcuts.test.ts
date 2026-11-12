import { afterEach, expect, test } from "vitest";
import {
  nextChord,
  notePointer,
  resolveShortcut,
  type ShortcutState,
} from "../shortcuts";

const idle: ShortcutState = { chord: false, singleKeys: true };

function press(init: KeyboardEventInit, target?: Element): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    ...init,
  });
  (target ?? document.body).dispatchEvent(event);
  return event;
}

afterEach(() => {
  document.body.innerHTML = "";
});

test.each([
  [{ key: " " }, "togglePlay"],
  [{ key: "ArrowRight", shiftKey: true }, "next"],
  [{ key: "ArrowLeft", shiftKey: true }, "previous"],
  [{ key: "m" }, "mute"],
  [{ key: "l" }, "like"],
  [{ key: "q" }, "queue"],
  [{ key: "n" }, "nowPlaying"],
  [{ key: "?", shiftKey: true }, "help"],
])("%j is the %s shortcut", (init, command) => {
  expect(resolveShortcut(press(init), idle)).toBe(command);
});

test("go to shortcuts need the G chord first", () => {
  expect(resolveShortcut(press({ key: "h" }), idle)).toBeNull();
  const chord: ShortcutState = { chord: true, singleKeys: true };
  expect(resolveShortcut(press({ key: "h" }), chord)).toBe("home");
  expect(resolveShortcut(press({ key: "l" }), chord)).toBe("library");
  expect(resolveShortcut(press({ key: "p" }), chord)).toBe("plugins");
  expect(nextChord(press({ key: "g" }), idle)).toBe(true);
  expect(nextChord(press({ key: "h" }), chord)).toBe(false);
});

test("shortcuts pause while typing in any field", () => {
  const field = document.createElement("input");
  const area = document.createElement("textarea");
  const editable = document.createElement("div");
  editable.setAttribute("contenteditable", "true");
  document.body.append(field, area, editable);
  for (const target of [field, area, editable])
    expect(resolveShortcut(press({ key: "m" }, target), idle)).toBeNull();
  expect(
    resolveShortcut(press({ key: "ArrowRight", shiftKey: true }, field), idle),
  ).toBeNull();
});

test("space leaves buttons, links and sliders reached from the keyboard to their own behaviour", () => {
  const button = document.createElement("button");
  const link = document.createElement("a");
  link.href = "/x";
  const slider = document.createElement("input");
  slider.type = "range";
  document.body.append(button, link, slider);
  for (const target of [button, link, slider])
    expect(resolveShortcut(press({ key: " " }, target), idle)).toBeNull();
});

test("space plays or pauses when the focused control was last pressed with a pointer", () => {
  const mute = document.createElement("button");
  mute.setAttribute("aria-label", "Mute");
  const icon = document.createElement("span");
  mute.append(icon);
  const seek = document.createElement("input");
  seek.type = "range";
  document.body.append(mute, seek);
  for (const [pressed, target] of [
    [icon, mute],
    [seek, seek],
  ] as const) {
    notePointer(pressed);
    expect(resolveShortcut(press({ key: " " }, target), idle)).toBe(
      "togglePlay",
    );
    expect(resolveShortcut(press({ key: " " }, target), idle)).toBe(
      "togglePlay",
    );
  }
});

test("moving focus with Tab hands space back to the control", () => {
  const next = document.createElement("button");
  document.body.append(next);
  notePointer(next);
  resolveShortcut(press({ key: "Tab" }, next), idle);
  expect(resolveShortcut(press({ key: " " }, next), idle)).toBeNull();
});

test("modifier combinations are never shortcuts", () => {
  expect(resolveShortcut(press({ key: "m", metaKey: true }), idle)).toBeNull();
  expect(resolveShortcut(press({ key: "m", ctrlKey: true }), idle)).toBeNull();
  expect(resolveShortcut(press({ key: "m", altKey: true }), idle)).toBeNull();
});

test("the single-key switch turns off letters and space but not help or Shift arrows", () => {
  const off: ShortcutState = { chord: false, singleKeys: false };
  expect(resolveShortcut(press({ key: "m" }), off)).toBeNull();
  expect(resolveShortcut(press({ key: " " }), off)).toBeNull();
  expect(resolveShortcut(press({ key: "?", shiftKey: true }), off)).toBe(
    "help",
  );
  expect(
    resolveShortcut(press({ key: "ArrowRight", shiftKey: true }), off),
  ).toBe("next");
});

test("an open dialog keeps every shortcut to itself", () => {
  const dialog = document.createElement("dialog");
  dialog.setAttribute("open", "");
  document.body.append(dialog);
  expect(resolveShortcut(press({ key: "m" }), idle)).toBeNull();
});
