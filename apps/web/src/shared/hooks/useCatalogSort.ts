import { router } from "@inertiajs/react";
import type { Schema } from "../schema";
import { log } from "../log";
import type { ListboxOption } from "../ui/Listbox";

export type CatalogSort = Schema<"CatalogSort">;
export type SortOption = ListboxOption & { value: CatalogSort };

export const DEFAULT_SORT: CatalogSort = "recent";

export const TRACK_SORTS: SortOption[] = [
  { value: "recent", label: "Recently added" },
  { value: "title", label: "Title" },
  { value: "artist", label: "Artist" },
];

export const ALBUM_SORTS: SortOption[] = [
  { value: "recent", label: "Recently added" },
  { value: "title", label: "Title" },
  { value: "artist", label: "Artist" },
  { value: "year", label: "Year" },
];

export const ARTIST_SORTS: SortOption[] = [
  { value: "recent", label: "Recently added" },
  { value: "title", label: "Name" },
];

const PAGING_PARAMETERS = [
  "offset",
  "recording_offset",
  "group_offset",
  "library_offset",
  "release_offset",
];

export function chosenSort(options: SortOption[]): CatalogSort {
  const asked = new URLSearchParams(window.location.search).get("sort");
  return (
    options.find((option) => option.value === asked)?.value ?? DEFAULT_SORT
  );
}

export function useCatalogSort(path: string, options: SortOption[]) {
  const sort = chosenSort(options);
  function change(value: string) {
    log.info("navigation.sort", { route: path, mode: value });
    const parameters = new URLSearchParams(window.location.search);
    PAGING_PARAMETERS.forEach((name) => parameters.delete(name));
    if (value === DEFAULT_SORT) parameters.delete("sort");
    else parameters.set("sort", value);
    router.get(path, Object.fromEntries(parameters), {
      preserveScroll: true,
      replace: true,
    });
  }
  return { sort, options, change };
}
