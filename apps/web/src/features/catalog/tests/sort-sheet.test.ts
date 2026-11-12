import { createElement } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import SortSheet from "../components/SortSheet";
import { TRACK_SORTS } from "../../../shared/hooks/useCatalogSort";

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false;
  };
});
afterEach(cleanup);

test("the sort sheet names the current sort, lists every sort and applies the one chosen", () => {
  const change = vi.fn();
  render(
    createElement(SortSheet, {
      options: TRACK_SORTS,
      value: TRACK_SORTS[0].value,
      onChange: change,
    }),
  );
  const trigger = screen.getByRole("button", {
    name: `Sort · ${TRACK_SORTS[0].label}`,
  });
  fireEvent.click(trigger);
  const group = screen.getByRole("group", { name: "Sort by" });
  expect(group.querySelectorAll("button")).toHaveLength(TRACK_SORTS.length);
  expect(
    screen
      .getByRole("button", { name: TRACK_SORTS[0].label })
      .getAttribute("aria-pressed"),
  ).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: TRACK_SORTS[1].label }));
  expect(change).toHaveBeenCalledWith(TRACK_SORTS[1].value);
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(screen.queryByRole("group", { name: "Sort by" })).toBeNull();
});
