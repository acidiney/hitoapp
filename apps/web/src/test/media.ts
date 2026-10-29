import { fireEvent, screen } from "@testing-library/react";

export function openSubscribeDialog() {
  fireEvent.click(
    screen.getByRole("button", { name: "Subscribe to a podcast" }),
  );
}

export function openAddAudiobookDialog() {
  fireEvent.click(
    screen.getByRole("button", { name: "Add audiobook from a playlist" }),
  );
}
