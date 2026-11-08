import { createElement, useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import SortControl from "../ui/SortControl";
import {
  ALBUM_SORTS,
  ARTIST_SORTS,
  TRACK_SORTS,
  chosenSort,
  useCatalogSort,
  type SortOption,
} from "../hooks/useCatalogSort";

const { get } = vi.hoisted(() => ({ get: vi.fn() }));
vi.mock("@inertiajs/react", () => ({ router: { get } }));

function Harness({ path, options }: { path: string; options: SortOption[] }) {
  const order = useCatalogSort(path, options);
  const [announced, announce] = useState("");
  return createElement(
    "div",
    null,
    createElement(SortControl, {
      options: order.options,
      value: order.sort,
      onChange: (value: string) => {
        announce(value);
        order.change(value);
      },
    }),
    createElement("output", null, `${order.sort}/${announced}`),
  );
}

function visit(search: string) {
  window.history.replaceState(null, "", `/albums${search}`);
}

beforeEach(() => get.mockClear());
afterEach(() => visit(""));

test("tracks, albums and artists offer their own sort choices, newest first by default", () => {
  expect(TRACK_SORTS.map((option) => option.label)).toEqual([
    "Recently added",
    "Title A–Z",
    "Artist A–Z",
  ]);
  expect(ALBUM_SORTS.map((option) => option.value)).toEqual([
    "recent",
    "title",
    "artist",
    "year",
  ]);
  expect(ARTIST_SORTS.map((option) => option.value)).toEqual([
    "recent",
    "title",
  ]);
  visit("");
  expect(chosenSort(ALBUM_SORTS)).toBe("recent");
});

test("an unknown or unsupported sort in the address falls back to recently added", () => {
  visit("?sort=oldest");
  expect(chosenSort(ALBUM_SORTS)).toBe("recent");
  visit("?sort=year");
  expect(chosenSort(ARTIST_SORTS)).toBe("recent");
  expect(chosenSort(ALBUM_SORTS)).toBe("year");
});

test("the sort control is a labelled listbox that shows the chosen sort", () => {
  visit("?sort=title");
  render(createElement(Harness, { path: "/albums", options: ALBUM_SORTS }));
  const control = screen.getByRole("combobox", { name: "Sort by" });
  expect(control.textContent).toBe("Title A–Z");
  fireEvent.click(control);
  expect(
    screen.getAllByRole("option").map((option) => option.textContent),
  ).toEqual([
    "Recently added",
    "Title A–Z",
    "Artist A–Z",
    "Year, newest first",
  ]);
  expect(document.querySelector("select")).toBeNull();
});

test("choosing a sort keeps the other filters, restarts paging and records the sort in the address", () => {
  visit("?q=moon&offset=60");
  render(createElement(Harness, { path: "/albums", options: ALBUM_SORTS }));
  fireEvent.click(screen.getByRole("combobox", { name: "Sort by" }));
  fireEvent.click(screen.getByRole("option", { name: "Year, newest first" }));
  expect(get).toHaveBeenCalledWith(
    "/albums",
    { q: "moon", sort: "year" },
    { preserveScroll: true, replace: true },
  );
});

test("choosing the default sort removes it from the address", () => {
  visit("?sort=artist&q=moon");
  render(createElement(Harness, { path: "/albums", options: ALBUM_SORTS }));
  fireEvent.click(screen.getByRole("combobox", { name: "Sort by" }));
  fireEvent.click(screen.getByRole("option", { name: "Recently added" }));
  expect(get).toHaveBeenCalledWith(
    "/albums",
    { q: "moon" },
    { preserveScroll: true, replace: true },
  );
});
