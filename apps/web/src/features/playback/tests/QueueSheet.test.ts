import { createElement } from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import QueueSheet from "../components/QueueSheet";
import { setQueueSheetOpen } from "../queueSheetState";

vi.mock("../components/QueueScreen", () => ({
  default: ({ onClose }: { onClose: () => void }) =>
    createElement("button", { onClick: onClose }, "Close queue"),
}));

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false;
  };
});
afterEach(() => {
  act(() => setQueueSheetOpen(false));
  cleanup();
});

test("the phone queue sheet opens over the current page and closes without navigating", () => {
  render(createElement(QueueSheet));
  expect(screen.queryByRole("dialog", { name: "Queue" })).toBeNull();
  act(() => setQueueSheetOpen(true));
  expect(screen.getByRole("dialog", { name: "Queue" })).toBeTruthy();
  act(() => screen.getByRole("button", { name: "Close queue" }).click());
  expect(screen.queryByRole("dialog", { name: "Queue" })).toBeNull();
});
