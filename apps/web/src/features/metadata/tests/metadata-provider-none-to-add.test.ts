import { render, screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { metadataPreferencesApi } from "../api/metadataPreferencesApi";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import {
  builtin,
  connections,
  preferences,
  settings,
} from "../fixtures/metadataPreferences";

vi.mock("../api/metadataPreferencesApi", () => ({
  metadataPreferencesApi: { settings: vi.fn(), update: vi.fn() },
}));
vi.mock("../../plugins/api/pluginsApi", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../plugins/api/pluginsApi")>();
  return { pluginsApi: { ...actual.pluginsApi, settings: vi.fn() } };
});
beforeEach(() => vi.resetAllMocks());

test("with nothing left to add the add row is replaced by a one line hint", async () => {
  vi.mocked(metadataPreferencesApi.settings).mockResolvedValue(
    settings([builtin], 3),
  );
  vi.mocked(pluginsApi.settings).mockResolvedValue(connections([]));
  render(preferences());
  expect(
    await screen.findByText("Every available metadata provider is in use."),
  ).toBeTruthy();
  expect(
    screen.queryByRole("combobox", { name: "Metadata provider" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Add metadata provider" }),
  ).toBeNull();
  expect(screen.queryByText("No available metadata providers")).toBeNull();
});
