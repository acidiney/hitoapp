import { fireEvent, screen } from "@testing-library/react";
import { editionGroup } from "./albumEditions";

export async function pickRelease() {
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose the release" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: `Choose ${editionGroup.title}` }),
  );
  await screen.findByRole("group", { name: /^Which track list\?/ });
}

export async function confirmEdition() {
  fireEvent.click(await screen.findByRole("button", { name: "Use this" }));
  return screen.findByRole("region", { name: "Album edition track list" });
}

export async function openFromAlbum() {
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  await pickRelease();
  return confirmEdition();
}
