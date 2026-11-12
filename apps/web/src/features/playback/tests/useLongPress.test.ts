import { createElement } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useLongPress } from "../hooks/useLongPress";

const picked = vi.fn();
const clicked = vi.fn();
function Row() {
  const press = useLongPress(picked);
  return createElement(
    "div",
    { ...press, "data-testid": "row" },
    createElement("button", { onClick: clicked }, "Play"),
  );
}
beforeEach(() => {
  vi.useFakeTimers();
  render(createElement(Row));
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllMocks();
});
const touch = (type: string, x = 0, y = 0) =>
  fireEvent(
    screen.getByTestId("row"),
    Object.assign(new Event(type, { bubbles: true }), {
      pointerType: "touch",
      clientX: x,
      clientY: y,
    }),
  );

test("holding a touch for half a second starts the selection and swallows the click that follows", () => {
  touch("pointerdown");
  vi.advanceTimersByTime(499);
  expect(picked).not.toHaveBeenCalled();
  vi.advanceTimersByTime(2);
  expect(picked).toHaveBeenCalledOnce();
  touch("pointerup");
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  expect(clicked).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  expect(clicked).toHaveBeenCalledOnce();
});

test("a short tap, a drag or a mouse press never starts a selection", () => {
  touch("pointerdown");
  vi.advanceTimersByTime(200);
  touch("pointerup");
  vi.advanceTimersByTime(600);
  touch("pointerdown");
  touch("pointermove", 0, 40);
  vi.advanceTimersByTime(600);
  fireEvent(
    screen.getByTestId("row"),
    Object.assign(new Event("pointerdown", { bubbles: true }), {
      pointerType: "mouse",
    }),
  );
  vi.advanceTimersByTime(600);
  expect(picked).not.toHaveBeenCalled();
});
