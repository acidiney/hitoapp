import { createElement } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
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

test("a grouped item opens its submenu, Back returns and ArrowLeft returns too", () => {
  const onSelect = vi.fn();
  render(
    createElement(ActionMenu, {
      label: "Actions",
      items: [
        { label: "Details", onSelect: vi.fn() },
        {
          label: "Add to playlist",
          items: [{ label: "Late night", onSelect }],
        },
      ],
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Actions" }));
  const group = screen.getByRole("menuitem", { name: "Add to playlist" });
  expect(group.getAttribute("aria-haspopup")).toBe("menu");
  fireEvent.click(group);
  const submenu = screen.getByRole("menu", { name: "Add to playlist" });
  expect(document.activeElement?.textContent).toBe("Add to playlist");
  expect(screen.queryByRole("menuitem", { name: "Details" })).toBeNull();
  fireEvent.keyDown(document.activeElement!, { key: "ArrowLeft" });
  expect(screen.getByRole("menuitem", { name: "Details" })).toBeTruthy();
  fireEvent.click(screen.getByRole("menuitem", { name: "Add to playlist" }));
  fireEvent.click(
    within(submenu).getByRole("menuitem", { name: "Late night" }),
  );
  expect(onSelect).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Actions" }),
  );
});

test("the menu can open to the end and above the trigger", () => {
  render(
    createElement(ActionMenu, {
      label: "Actions",
      align: "end",
      items: [{ label: "Details", onSelect: vi.fn() }],
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Actions" }));
  const menu = screen.getByRole("menu");
  expect(menu.parentElement).toBe(document.body);
  expect(menu.style.position).toBe("fixed");
  expect(["above", "below"]).toContain(menu.getAttribute("data-placement"));
});
