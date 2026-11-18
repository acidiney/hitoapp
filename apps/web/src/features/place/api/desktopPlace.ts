import { invoke } from "@tauri-apps/api/core";

export const desktopPlace = {
  useThisComputer: () => invoke<void>("use_this_computer"),
};
