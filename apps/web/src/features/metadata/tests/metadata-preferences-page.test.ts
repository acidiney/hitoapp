import { createElement } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import SettingsPage from "../../../pages/SettingsPage";
import { metadataPreferencesApi } from "../api/metadataPreferencesApi";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import { connections, settings } from "../fixtures/metadataPreferences";
import type { SettingsPageProps } from "../../../shared/types";

vi.mock("@inertiajs/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@inertiajs/react")>();
  return { ...actual, usePage: () => ({ url: "/settings?section=metadata" }) };
});
vi.mock("../api/metadataPreferencesApi", () => ({
  metadataPreferencesApi: { settings: vi.fn(), update: vi.fn() },
}));
vi.mock("../../plugins/api/pluginsApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../plugins/api/pluginsApi")>();
  return { pluginsApi: { ...actual.pluginsApi, settings: vi.fn() } };
});

const props: SettingsPageProps = {
  user: { id: "alice", username: "Alice", admin: true },
  errors: {},
  native_playback: false,
  libraries: [],
  playlists: [],
};
const unexpectedRequest = vi.fn<typeof fetch>();
beforeEach(() => {
  vi.resetAllMocks();
  unexpectedRequest.mockRejectedValue(
    new Error("Settings navigation must not request provider operations"),
  );
  vi.stubGlobal("fetch", unexpectedRequest);
});
test("the Metadata settings deep link opens the real account preferences and retains keyboard tab navigation", async () => {
  const read = Promise.resolve(settings());
  const pluginRead = Promise.resolve(connections());
  vi.mocked(metadataPreferencesApi.settings).mockReturnValue(read);
  vi.mocked(pluginsApi.settings).mockReturnValue(pluginRead);
  const view = render(createElement(SettingsPage, props));
  try {
    const metadata = screen.getByRole("tab", { name: "Metadata" });
    const tabs = screen.getAllByRole("tab");
    expect(
      tabs.filter((tab) => tab !== metadata).map((tab) => tab.textContent),
    ).toEqual(["Library", "Privacy", "Plugins", "Accounts"]);
    expect(metadata.getAttribute("aria-selected")).toBe("true");
    expect(metadata.tabIndex).toBe(0);
    for (const tab of tabs.filter((tab) => tab !== metadata))
      expect(tab.tabIndex).toBe(-1);
    const panel = screen.getByRole("tabpanel", { name: "Metadata" });
    expect(metadata.getAttribute("aria-controls")).toBe(panel.id);
    expect(panel.getAttribute("aria-labelledby")).toBe(metadata.id);
    const enabled = (await within(panel).findByRole("checkbox", {
      name: "Use metadata providers",
    })) as HTMLInputElement;
    expect(enabled.checked).toBe(true);
    expect(
      within(panel).getByText("MusicBrainz", { exact: true }),
    ).toBeTruthy();
    expect(metadataPreferencesApi.settings).toHaveBeenCalledExactlyOnceWith(
      props.user.id,
      expect.any(AbortSignal),
    );
    expect(pluginsApi.settings).toHaveBeenCalledExactlyOnceWith(
      props.user.id,
      expect.any(AbortSignal),
    );
    const firstRead = vi.mocked(metadataPreferencesApi.settings).mock
      .calls[0][1];
    const firstPlugins = vi.mocked(pluginsApi.settings).mock.calls[0][1];

    metadata.focus();
    fireEvent.keyDown(metadata, { key: "Home" });
    const library = screen.getByRole("tab", { name: "Library" });
    expect(document.activeElement).toBe(library);
    expect(library.getAttribute("aria-selected")).toBe("true");
    expect(library.tabIndex).toBe(0);
    expect(metadata.getAttribute("aria-selected")).toBe("false");
    expect(metadata.tabIndex).toBe(-1);
    expect(screen.getByRole("tabpanel", { name: "Library" })).toBeTruthy();
    expect(
      screen.queryByRole("checkbox", { name: "Use metadata providers" }),
    ).toBeNull();
    expect(firstRead.aborted && firstPlugins.aborted).toBe(true);

    fireEvent.click(metadata);
    const reopened = screen.getByRole("tabpanel", { name: "Metadata" });
    await within(reopened).findByRole("checkbox", {
      name: "Use metadata providers",
    });
    expect(metadataPreferencesApi.settings).toHaveBeenCalledTimes(2);
    expect(pluginsApi.settings).toHaveBeenCalledTimes(2);
    expect(metadataPreferencesApi.update).not.toHaveBeenCalled();
    expect(unexpectedRequest).not.toHaveBeenCalled();
    view.unmount();
    expect(
      vi.mocked(metadataPreferencesApi.settings).mock.calls[1][1].aborted,
    ).toBe(true);
    expect(vi.mocked(pluginsApi.settings).mock.calls[1][1].aborted).toBe(true);
  } finally {
    view.unmount();
    await act(async () => {
      await Promise.all([read, pluginRead]);
    });
  }
});
