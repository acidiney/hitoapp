import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import EditionToolbar from "../components/EditionToolbar";
import { editionChip, editionOptions } from "../../../test/editionMenu";

function toolbar(owned: number) {
  const onFilter = vi.fn();
  const choose = vi.fn();
  render(
    createElement(EditionToolbar, {
      name: "Deluxe · 2025",
      total: 12,
      owned,
      filter: "all",
      onFilter,
      source: "MusicBrainz",
      items: [
        { id: "a", label: "Deluxe · 2025", onSelect: choose },
        { id: "b", label: "Deluxe · 2025", onSelect: choose },
      ],
    }),
  );
  return { onFilter, choose };
}

test("nothing owned hides the All and In my library chips but keeps the edition and its source", () => {
  toolbar(0);
  expect(screen.queryByRole("button", { name: /^All / })).toBeNull();
  expect(screen.queryByRole("button", { name: /^In my library/ })).toBeNull();
  expect(editionChip()?.textContent).toContain("Deluxe · 2025");
  expect(screen.getByText("Track list from MusicBrainz")).toBeTruthy();
});

test("owned tracks show the filter chips with their counts", () => {
  const { onFilter } = toolbar(3);
  expect(
    screen.getByRole("button", { name: "All 12" }).getAttribute("aria-pressed"),
  ).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "In my library 3" }));
  expect(onFilter).toHaveBeenCalledWith("library");
});

test("editions with the same label stay separate menu items", () => {
  toolbar(1);
  expect(editionOptions()).toEqual(["Deluxe · 2025", "Deluxe · 2025"]);
});
