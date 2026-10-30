import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ActionMenu from "../ui/ActionMenu";

function setup() {
  const onSelect = vi.fn();
  render(
    createElement(ActionMenu, {
      label: "More options",
      items: [{ label: "Delete playlist", destructive: true, onSelect }],
    }),
  );
  return {
    onSelect,
    trigger: screen.getByRole("button", { name: "More options" }),
  };
}

test("opening the menu moves focus to its first item", () => {
  const { trigger } = setup();
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(trigger);
  expect(trigger.getAttribute("aria-expanded")).toBe("true");
  expect(document.activeElement).toBe(
    screen.getByRole("menuitem", { name: "Delete playlist" }),
  );
});

test("Escape closes the menu and returns focus to its button", () => {
  const { trigger } = setup();
  fireEvent.click(trigger);
  fireEvent.keyDown(screen.getByRole("menuitem"), { key: "Escape" });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(trigger);
});

test("choosing an item closes the menu and runs it once", () => {
  const { trigger, onSelect } = setup();
  fireEvent.click(trigger);
  fireEvent.click(screen.getByRole("menuitem", { name: "Delete playlist" }));
  expect(onSelect).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(trigger);
});

test("a press outside closes the menu", () => {
  const { trigger } = setup();
  fireEvent.click(trigger);
  fireEvent.pointerDown(document.body);
  expect(screen.queryByRole("menu")).toBeNull();
});
