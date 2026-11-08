import { fireEvent, screen } from "@testing-library/react";

export function openImport() {
  fireEvent.click(screen.getByRole("button", { name: "Import from a link" }));
}

export function openCreate() {
  fireEvent.click(screen.getByRole("button", { name: "New playlist" }));
}
