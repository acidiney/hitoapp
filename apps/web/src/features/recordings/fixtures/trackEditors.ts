import { fireEvent, screen } from "@testing-library/react";

export function openTrackEditors() {
  for (const field of ["title", "artist"]) {
    if (screen.queryByRole("textbox", { name: `Track ${field}` })) continue;
    fireEvent.click(
      screen.getByRole("button", { name: `Edit track ${field}` }),
    );
  }
}
