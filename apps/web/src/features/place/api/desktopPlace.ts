import { invoke } from "@tauri-apps/api/core";

export const desktopPlace = {
  startHere: () => invoke<void>("use_this_computer"),
};
