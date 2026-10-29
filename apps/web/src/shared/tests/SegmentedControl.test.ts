import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import SegmentedControl from "../ui/SegmentedControl";
import FilterPills from "../ui/FilterPills";

const options = [
  { value: "never", label: "Never" },
  { value: "15", label: "15 min" },
  { value: "60", label: "1 hour" },
];

function segmented(value: string, onChange = vi.fn()) {
  render(
    createElement(SegmentedControl, {
      label: "Lock timeout",
      options,
      value,
      onChange,
    }),
  );
  return onChange;
}

test("a segmented control is a labelled radio group with one checked option", () => {
  segmented("15");
  const group = screen.getByRole("radiogroup", { name: "Lock timeout" });
  expect(group).toBeTruthy();
  const radios = screen.getAllByRole("radio");
  expect(radios.map((radio) => radio.getAttribute("aria-checked"))).toEqual([
    "false",
    "true",
    "false",
  ]);
  expect(radios.map((radio) => radio.getAttribute("tabindex"))).toEqual([
    "-1",
    "0",
    "-1",
  ]);
});

test("clicking an option chooses it", () => {
  const onChange = segmented("15");
  fireEvent.click(screen.getByRole("radio", { name: "1 hour" }));
  expect(onChange).toHaveBeenCalledWith("60");
});

test("arrow keys move the choice and wrap around the ends", () => {
  const onChange = segmented("15");
  const current = screen.getByRole("radio", { name: "15 min" });
  fireEvent.keyDown(current, { key: "ArrowRight" });
  expect(onChange).toHaveBeenLastCalledWith("60");
  fireEvent.keyDown(current, { key: "ArrowLeft" });
  expect(onChange).toHaveBeenLastCalledWith("never");
  fireEvent.keyDown(screen.getByRole("radio", { name: "1 hour" }), {
    key: "ArrowDown",
  });
  expect(onChange).toHaveBeenLastCalledWith("never");
  fireEvent.keyDown(screen.getByRole("radio", { name: "Never" }), {
    key: "ArrowUp",
  });
  expect(onChange).toHaveBeenLastCalledWith("60");
});

test("Home and End jump to the first and last option", () => {
  const onChange = segmented("15");
  const current = screen.getByRole("radio", { name: "15 min" });
  fireEvent.keyDown(current, { key: "End" });
  expect(onChange).toHaveBeenLastCalledWith("60");
  fireEvent.keyDown(current, { key: "Home" });
  expect(onChange).toHaveBeenLastCalledWith("never");
});

test("filter pills report their pressed state and choose one filter", () => {
  const onChange = vi.fn();
  render(
    createElement(FilterPills, {
      label: "Show",
      options: [
        { value: "albums", label: "Albums" },
        { value: "artists", label: "Artists" },
      ],
      value: "albums",
      onChange,
    }),
  );
  const group = screen.getByRole("group", { name: "Show" });
  expect(group).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "Albums" }).getAttribute("aria-pressed"),
  ).toBe("true");
  expect(
    screen
      .getByRole("button", { name: "Artists" })
      .getAttribute("aria-pressed"),
  ).toBe("false");
  fireEvent.click(screen.getByRole("button", { name: "Artists" }));
  expect(onChange).toHaveBeenCalledWith("artists");
});

test("when the value matches no option the first option is still reachable by Tab", () => {
  segmented("missing");
  expect(
    screen.getAllByRole("radio").map((radio) => radio.getAttribute("tabindex")),
  ).toEqual(["0", "-1", "-1"]);
});
