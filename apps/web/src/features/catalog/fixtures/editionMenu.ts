import { fireEvent, screen } from "@testing-library/react";

export const editionChip = () =>
  screen.getByRole("button", { name: /^Edition / });

export function editionMenuItem(name: string) {
  fireEvent.click(editionChip());
  return screen.getByRole("menuitem", { name });
}
