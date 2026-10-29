import { isOn } from "../../../test/switch";
import { createElement } from "react";
import { act, render, screen, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { NoticeProvider } from "../../../shared/ui/Notices";
import SettingsPage from "../../settings/pages/SettingsPage";
import { metadataPreferencesApi } from "../api/metadataPreferencesApi";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import { connections, settings } from "../fixtures/metadataPreferences";
import type { SettingsPageProps } from "../../../shared/types";

const location = vi.hoisted(() => ({ url: "/settings?section=metadata" }));
vi.mock("@inertiajs/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@inertiajs/react")>();
  return { ...actual, usePage: () => ({ url: location.url }) };
});
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({ clear: vi.fn() }),
}));
vi.mock("../api/metadataPreferencesApi", () => ({
  metadataPreferencesApi: { settings: vi.fn(), update: vi.fn() },
}));
vi.mock("../../plugins/api/pluginsApi", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../plugins/api/pluginsApi")>();
  return { pluginsApi: { ...actual.pluginsApi, settings: vi.fn() } };
});

const props: SettingsPageProps = {
  user: { id: "alice", username: "Alice", admin: true },
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
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
test("the legacy Metadata deep link opens the real account preferences inside Music & providers", async () => {
  const read = Promise.resolve(settings());
  const pluginRead = Promise.resolve(connections());
  vi.mocked(metadataPreferencesApi.settings).mockReturnValue(read);
  vi.mocked(pluginsApi.settings).mockReturnValue(pluginRead);
  location.url = "/settings?section=metadata";
  const view = render(
    createElement(NoticeProvider, null, createElement(SettingsPage, props)),
  );
  const show = (url: string) => {
    location.url = url;
    view.rerender(
      createElement(NoticeProvider, null, createElement(SettingsPage, props)),
    );
  };
  try {
    const nav = screen.getByRole("navigation", { name: "Settings sections" });
    expect(
      within(nav)
        .getAllByRole("link")
        .filter((link) => link.getAttribute("aria-current") === "page")
        .map((link) => link.textContent),
    ).toEqual(["Music & providers"]);
    const panel = screen.getByRole("region", { name: "Metadata providers" });
    const enabled = (await within(panel).findByRole("switch", {
      name: "Use metadata providers",
    })) as HTMLButtonElement;
    expect(isOn(enabled)).toBe(true);
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

    show("/settings?section=about");
    expect(
      screen.queryByRole("switch", { name: "Use metadata providers" }),
    ).toBeNull();
    expect(firstRead.aborted && firstPlugins.aborted).toBe(true);

    show("/settings?section=metadata");
    await within(
      screen.getByRole("region", { name: "Metadata providers" }),
    ).findByRole("switch", { name: "Use metadata providers" });
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
