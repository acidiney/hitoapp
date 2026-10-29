import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import Switch from "../ui/Switch";

function view(checked: boolean, onChange = vi.fn(), disabled = false) {
  render(
    createElement(Switch, {
      label: "Download new",
      hint: "Fetch new episodes automatically",
      checked,
      onChange,
      disabled,
    }),
  );
  return onChange;
}

test("a switch exposes its state, name and hint to assistive technology", () => {
  view(true);
  const control = screen.getByRole("switch", { name: "Download new" });
  expect(control.getAttribute("aria-checked")).toBe("true");
  expect(control.getAttribute("aria-describedby")).toBeTruthy();
  expect(
    document.getElementById(control.getAttribute("aria-describedby") ?? "")
      ?.textContent,
  ).toBe("Fetch new episodes automatically");
});

test("activating a switch asks for the opposite state immediately", () => {
  const onChange = view(false);
  const control = screen.getByRole("switch", { name: "Download new" });
  expect(control.getAttribute("aria-checked")).toBe("false");
  fireEvent.click(control);
  expect(onChange).toHaveBeenCalledWith(true);
});

test("a switch is a real button so Space and Enter reach it from the keyboard", () => {
  view(false);
  const control = screen.getByRole("switch", { name: "Download new" });
  expect(control.tagName).toBe("BUTTON");
  expect(control.getAttribute("type")).toBe("button");
});

test("a disabled switch ignores activation", () => {
  const onChange = view(false, vi.fn(), true);
  fireEvent.click(screen.getByRole("switch", { name: "Download new" }));
  expect(onChange).not.toHaveBeenCalled();
});
