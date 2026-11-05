import { createElement, useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import Listbox from "./Listbox";

const options = [
  { value: "home", label: "Home library" },
  { value: "archive", label: "Archive" },
  { value: "attic", label: "Attic recordings" },
  { value: "studio", label: "Studio" },
];

function Host({
  onChange = () => {},
  disabled = false,
}: {
  onChange?: (value: string) => void;
  disabled?: boolean;
}) {
  const [value, setValue] = useState("home");
  return createElement(
    "div",
    null,
    createElement(Listbox, {
      label: "Library",
      options,
      value,
      disabled,
      onChange: (next: string) => {
        onChange(next);
        setValue(next);
      },
    }),
    createElement("button", null, "After"),
  );
}

function combobox() {
  const control = screen.getByRole("combobox", { name: "Library" });
  control.focus();
  return control;
}

test("the closed control is a labelled combobox showing the selected option", () => {
  render(createElement(Host));
  const control = combobox();
  expect(control.textContent).toContain("Home library");
  expect(control.getAttribute("aria-haspopup")).toBe("listbox");
  expect(control.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("opening exposes a labelled listbox with the selected option active", () => {
  render(createElement(Host));
  const control = combobox();
  fireEvent.click(control);
  const list = screen.getByRole("listbox", { name: "Library" });
  expect(control.getAttribute("aria-expanded")).toBe("true");
  expect(control.getAttribute("aria-controls")).toBe(list.id);
  const items = screen.getAllByRole("option");
  expect(items.map((item) => item.textContent)).toEqual(
    options.map((option) => option.label),
  );
  expect(items[0].getAttribute("aria-selected")).toBe("true");
  expect(items[1].getAttribute("aria-selected")).toBe("false");
  expect(control.getAttribute("aria-activedescendant")).toBe(items[0].id);
  expect(document.activeElement).toBe(control);
});

test("arrow keys move the active option and Enter commits it", () => {
  const onChange = vi.fn();
  render(createElement(Host, { onChange }));
  const control = combobox();
  fireEvent.keyDown(control, { key: "ArrowDown" });
  const items = screen.getAllByRole("option");
  expect(control.getAttribute("aria-activedescendant")).toBe(items[0].id);
  fireEvent.keyDown(control, { key: "ArrowDown" });
  fireEvent.keyDown(control, { key: "ArrowDown" });
  expect(control.getAttribute("aria-activedescendant")).toBe(items[2].id);
  fireEvent.keyDown(control, { key: "ArrowUp" });
  expect(control.getAttribute("aria-activedescendant")).toBe(items[1].id);
  expect(onChange).not.toHaveBeenCalled();
  fireEvent.keyDown(control, { key: "Enter" });
  expect(onChange).toHaveBeenCalledWith("archive");
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(control.textContent).toContain("Archive");
  expect(document.activeElement).toBe(control);
});

test("arrows stop at the first and last option", () => {
  render(createElement(Host));
  const control = combobox();
  fireEvent.keyDown(control, { key: "ArrowDown" });
  const items = screen.getAllByRole("option");
  fireEvent.keyDown(control, { key: "ArrowUp" });
  expect(control.getAttribute("aria-activedescendant")).toBe(items[0].id);
  fireEvent.keyDown(control, { key: "End" });
  expect(control.getAttribute("aria-activedescendant")).toBe(items[3].id);
  fireEvent.keyDown(control, { key: "ArrowDown" });
  expect(control.getAttribute("aria-activedescendant")).toBe(items[3].id);
  fireEvent.keyDown(control, { key: "Home" });
  expect(control.getAttribute("aria-activedescendant")).toBe(items[0].id);
});

test("Home and End on the closed control open it on the first and last option", () => {
  render(createElement(Host));
  const control = combobox();
  fireEvent.keyDown(control, { key: "End" });
  const items = screen.getAllByRole("option");
  expect(control.getAttribute("aria-activedescendant")).toBe(items[3].id);
});

test("Escape closes without changing the value and keeps focus", () => {
  const onChange = vi.fn();
  render(createElement(Host, { onChange }));
  const control = combobox();
  fireEvent.keyDown(control, { key: "ArrowDown" });
  fireEvent.keyDown(control, { key: "ArrowDown" });
  fireEvent.keyDown(control, { key: "Escape" });
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(onChange).not.toHaveBeenCalled();
  expect(control.textContent).toContain("Home library");
  expect(document.activeElement).toBe(control);
});

test("Escape is consumed while the list is open so an enclosing dialog stays open", () => {
  render(createElement(Host));
  const control = combobox();
  fireEvent.keyDown(control, { key: "ArrowDown" });
  expect(fireEvent.keyDown(control, { key: "Escape" })).toBe(false);
  expect(fireEvent.keyDown(control, { key: "Escape" })).toBe(true);
});

test("typing letters moves to the matching option and repeating cycles", () => {
  render(createElement(Host));
  const control = combobox();
  fireEvent.keyDown(control, { key: "ArrowDown" });
  const items = screen.getAllByRole("option");
  fireEvent.keyDown(control, { key: "a" });
  expect(control.getAttribute("aria-activedescendant")).toBe(items[1].id);
  fireEvent.keyDown(control, { key: "a" });
  expect(control.getAttribute("aria-activedescendant")).toBe(items[2].id);
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(Date.now() + 2000);
  fireEvent.keyDown(control, { key: "s" });
  vi.useRealTimers();
  expect(control.getAttribute("aria-activedescendant")).toBe(items[3].id);
});

test("typing several letters quickly matches the whole prefix", () => {
  render(createElement(Host));
  const control = combobox();
  fireEvent.keyDown(control, { key: "ArrowDown" });
  const items = screen.getAllByRole("option");
  fireEvent.keyDown(control, { key: "a" });
  fireEvent.keyDown(control, { key: "t" });
  fireEvent.keyDown(control, { key: "t" });
  expect(control.getAttribute("aria-activedescendant")).toBe(items[2].id);
});

test("clicking an option commits it and closes the list", () => {
  const onChange = vi.fn();
  render(createElement(Host, { onChange }));
  fireEvent.click(combobox());
  fireEvent.click(screen.getByRole("option", { name: "Studio" }));
  expect(onChange).toHaveBeenCalledWith("studio");
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(combobox().textContent).toContain("Studio");
});

test("clicking the control again closes the open list", () => {
  render(createElement(Host));
  fireEvent.click(combobox());
  fireEvent.click(combobox());
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("moving focus away closes the list without changing the value", () => {
  const onChange = vi.fn();
  render(createElement(Host, { onChange }));
  const control = combobox();
  fireEvent.click(control);
  fireEvent.blur(control);
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(onChange).not.toHaveBeenCalled();
});

test("Tab commits the active option and lets focus move on", () => {
  const onChange = vi.fn();
  render(createElement(Host, { onChange }));
  const control = combobox();
  fireEvent.keyDown(control, { key: "ArrowDown" });
  fireEvent.keyDown(control, { key: "ArrowDown" });
  fireEvent.keyDown(control, { key: "Tab" });
  expect(onChange).toHaveBeenCalledWith("archive");
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("a disabled control cannot be opened", () => {
  render(createElement(Host, { disabled: true }));
  const control = combobox();
  expect(control.hasAttribute("disabled")).toBe(true);
  fireEvent.click(control);
  fireEvent.keyDown(control, { key: "ArrowDown" });
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("a value that is not offered shows the first option as the current choice", () => {
  render(
    createElement(Listbox, {
      label: "Library",
      options,
      value: "missing",
      onChange: () => {},
    }),
  );
  expect(combobox().textContent).toContain("Home library");
});
