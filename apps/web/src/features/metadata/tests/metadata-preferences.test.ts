import { createElement } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import MetadataPreferences from "../components/MetadataPreferences";
import { metadataPreferencesApi } from "../api/metadataPreferencesApi";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import type { MetadataSettings, PluginSettings } from "../../../shared/types";

vi.mock("../api/metadataPreferencesApi", () => ({
  metadataPreferencesApi: { settings: vi.fn(), update: vi.fn() },
}));
vi.mock("../../plugins/api/pluginsApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../plugins/api/pluginsApi")>();
  return { pluginsApi: { ...actual.pluginsApi, settings: vi.fn() } };
});

const initial: MetadataSettings = {
  revision: 0,
  enabled: true,
  providers: [{ kind: "builtin_musicbrainz" }],
};
const connections: PluginSettings = {
  credential_storage_ready: true,
  services: [],
  installations: [],
};
const accepted: MetadataSettings = { ...initial, revision: 1, enabled: false };
const unexpectedRequest = vi.fn<typeof fetch>();

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(metadataPreferencesApi.settings).mockResolvedValue(initial);
  vi.mocked(metadataPreferencesApi.update).mockResolvedValue(accepted);
  vi.mocked(pluginsApi.settings).mockResolvedValue(connections);
  unexpectedRequest.mockRejectedValue(
    new Error(
      "Metadata preferences must not request artist or provider operations",
    ),
  );
  vi.stubGlobal("fetch", unexpectedRequest);
});
test("the built-in metadata default can be disabled without connecting or querying a provider", async () => {
  render(createElement(MetadataPreferences, { account: "alice" }));
  const enabled = (await screen.findByRole("checkbox", {
    name: "Use metadata providers",
  })) as HTMLInputElement;
  expect(enabled.checked).toBe(true);
  const selected = screen.getByRole("list", {
    name: "Selected metadata providers",
  });
  expect(
    within(selected).getByText("MusicBrainz", { exact: true }),
  ).toBeTruthy();
  expect(pluginsApi.settings).toHaveBeenCalledExactlyOnceWith(
    "alice",
    expect.any(AbortSignal),
  );
  expect(metadataPreferencesApi.settings).toHaveBeenCalledExactlyOnceWith(
    "alice",
    expect.any(AbortSignal),
  );
  expect(screen.queryByRole("button", { name: "Connect service" })).toBeNull();
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(document.querySelector('input[type="password"]')).toBeNull();
  expect(metadataPreferencesApi.update).not.toHaveBeenCalled();
  expect(unexpectedRequest).not.toHaveBeenCalled();

  fireEvent.click(enabled);
  expect(enabled.checked).toBe(false);
  expect(metadataPreferencesApi.update).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Save metadata preferences",
    }),
  );
  await waitFor(() =>
    expect(metadataPreferencesApi.update).toHaveBeenCalledExactlyOnceWith(
      "alice",
      { revision: 0, enabled: false, providers: initial.providers },
      expect.any(AbortSignal),
    ),
  );
  await screen.findByText("Metadata preferences saved.", { exact: true });
  expect(enabled.checked).toBe(accepted.enabled);
  expect(
    (
      screen.getByRole("button", {
        name: "Save metadata preferences",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  expect(
    within(selected).getByText("MusicBrainz", { exact: true }),
  ).toBeTruthy();
  expect(metadataPreferencesApi.settings).toHaveBeenCalledTimes(1);
  expect(pluginsApi.settings).toHaveBeenCalledTimes(1);
  expect(unexpectedRequest).not.toHaveBeenCalled();
});
