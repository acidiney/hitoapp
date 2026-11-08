import { createElement } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ShelfRow from "../ui/ShelfRow";

afterEach(cleanup);

function shelf() {
  render(
    createElement(ShelfRow, {
      label: "Recently added",
      children: [
        createElement("li", { key: "a" }, "A"),
        createElement("li", { key: "b" }, "B"),
      ],
    }),
  );
  return document.querySelector("ul.shelf-row") as HTMLElement;
}

function measure(
  element: HTMLElement,
  sizes: { scrollWidth: number; clientWidth: number; scrollLeft: number },
) {
  for (const [name, value] of Object.entries(sizes))
    Object.defineProperty(element, name, { value, configurable: true });
  fireEvent.scroll(element);
}

const next = () =>
  screen.queryByRole("button", { name: "Next in Recently added" });
const previous = () =>
  screen.queryByRole("button", { name: "Previous in Recently added" });

test("a row that fits has no buttons and no fade", () => {
  const row = shelf();
  measure(row, { scrollWidth: 300, clientWidth: 300, scrollLeft: 0 });
  expect(next()).toBeNull();
  expect(previous()).toBeNull();
  expect(row.hasAttribute("data-fade-end")).toBe(false);
});

test("a clipped row fades its hidden edge and offers the button for that direction only", () => {
  const row = shelf();
  measure(row, { scrollWidth: 900, clientWidth: 400, scrollLeft: 0 });
  expect(row.hasAttribute("data-fade-end")).toBe(true);
  expect(next()).toBeTruthy();
  expect(previous()).toBeNull();
  measure(row, { scrollWidth: 900, clientWidth: 400, scrollLeft: 200 });
  expect(next()).toBeTruthy();
  expect(previous()).toBeTruthy();
  measure(row, { scrollWidth: 900, clientWidth: 400, scrollLeft: 500 });
  expect(next()).toBeNull();
  expect(previous()).toBeTruthy();
});

test("the buttons scroll by most of a row width", () => {
  const row = shelf();
  const scrollBy = vi.fn();
  row.scrollBy = scrollBy;
  measure(row, { scrollWidth: 900, clientWidth: 400, scrollLeft: 200 });
  fireEvent.click(next() as HTMLElement);
  expect(scrollBy).toHaveBeenLastCalledWith(
    expect.objectContaining({ left: 320 }),
  );
  fireEvent.click(previous() as HTMLElement);
  expect(scrollBy).toHaveBeenLastCalledWith(
    expect.objectContaining({ left: -320 }),
  );
});
