import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { expect } from "vitest";
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

export async function loadedTrackList() {
  const region = await screen.findByRole("region", {
    name: "Album edition track list",
  });
  await within(region).findAllByRole("listitem");
  return region;
}

export async function confirmEdition() {
  const use = await screen.findByRole("button", { name: "Use this" });
  await waitFor(() => expect(use.hasAttribute("disabled")).toBe(false));
  fireEvent.click(use);
  return loadedTrackList();
}

export async function openFromAlbum() {
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose an edition" }),
  );
  await pickRelease();
  return confirmEdition();
}
