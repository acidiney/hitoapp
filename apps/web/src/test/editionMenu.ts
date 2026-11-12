import { fireEvent, screen } from "@testing-library/react";

const LOAD_MORE = "Load more editions";

export function editionChip() {
  return screen.queryByRole("button", { name: /^Edition / });
}

function open() {
  const chip = editionChip();
  if (!chip) throw new Error("No edition chip");
  if (chip.getAttribute("aria-expanded") !== "true") fireEvent.click(chip);
  return screen.getAllByRole("menuitem");
}

function close() {
  const menu = screen.queryByRole("menu");
  if (menu) fireEvent.keyDown(menu, { key: "Escape" });
}

export function editionOptions() {
  const labels = open()
    .map((item) => item.textContent ?? "")
    .filter((label) => label !== LOAD_MORE);
  close();
  return labels;
}

export function canLoadMoreEditions() {
  if (!editionChip()) return false;
  const more = open().find((item) => item.textContent === LOAD_MORE);
  const usable = more !== undefined && !(more as HTMLButtonElement).disabled;
  close();
  return usable;
}

export function chooseEdition(option: string | number) {
  const items = open().filter((item) => item.textContent !== LOAD_MORE);
  const target =
    typeof option === "number"
      ? items[option]
      : items.find((item) => item.textContent === option);
  if (!target) throw new Error(`No edition ${String(option)}`);
  fireEvent.click(target);
}

export function loadMoreEditions() {
  const more = open().find((item) => item.textContent === LOAD_MORE);
  if (!more) throw new Error("No Load more editions item");
  fireEvent.click(more);
}
