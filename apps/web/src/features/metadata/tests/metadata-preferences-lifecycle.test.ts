import { isOn } from "../../../test/switch";
import { createElement, StrictMode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { metadataPreferencesApi } from "../api/metadataPreferencesApi";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import { HttpError } from "../../../shared/http";
import {
  builtin,
  catalog,
  connections,
  held,
  preferences,
  reference,
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
const alice = catalog(1, "Alice private catalog");
const unexpectedRequest = vi.fn<typeof fetch>();
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(pluginsApi.settings).mockResolvedValue(connections([alice]));
  unexpectedRequest.mockRejectedValue(
    new Error("Metadata preferences must not contact provider operations"),
  );
  vi.stubGlobal("fetch", unexpectedRequest);
});
function enabled() {
  return screen.getByRole("switch", {
    name: "Use metadata providers",
  }) as HTMLButtonElement;
}
function save() {
  return screen.getByRole("button", {
    name: "Save metadata preferences",
  });
}
function selected() {
  return screen.getByRole("list", {
    name: "Selected metadata providers",
  });
}

test("a retired initial read cannot overwrite an accepted save and duplicate submission makes one write", async () => {
  const initial = held(settings());
  const current = settings([reference(alice)], 5);
  const accepted = settings(current.providers, 6, false);
  const update = held(accepted);
  vi.mocked(metadataPreferencesApi.settings)
    .mockReturnValueOnce(initial.promise)
    .mockResolvedValue(current);
  vi.mocked(metadataPreferencesApi.update).mockReturnValue(update.promise);
  const view = render(createElement(StrictMode, null, preferences()));
  try {
    await screen.findByText(alice.service.name, { exact: true });
    expect(
      vi.mocked(metadataPreferencesApi.settings).mock.calls[0][1].aborted,
    ).toBe(true);
    fireEvent.click(enabled());
    const form = save().closest("form");
    expect(form).not.toBeNull();
    act(() => {
      fireEvent.submit(form!);
      fireEvent.submit(form!);
    });
    expect(metadataPreferencesApi.update).toHaveBeenCalledExactlyOnceWith(
      "alice",
      { revision: 5, enabled: false, providers: current.providers },
      expect.any(AbortSignal),
    );
    await act(async () => {
      update.resolve(accepted);
      await update.promise;
    });
    await screen.findByText("Metadata preferences saved.", { exact: true });
    await act(async () => {
      initial.resolve(settings());
      await initial.promise;
    });
    expect(isOn(enabled())).toBe(false);
    expect(
      within(selected()).getByText(alice.service.name, { exact: true }),
    ).toBeTruthy();
    expect(
      within(selected()).queryByText("MusicBrainz", { exact: true }),
    ).toBeNull();
    expect(metadataPreferencesApi.update).toHaveBeenCalledTimes(1);
    expect(unexpectedRequest).not.toHaveBeenCalled();
  } finally {
    view.unmount();
    await act(async () => {
      await Promise.all([initial.finish(), update.finish()]);
    });
  }
});

test("account changes fence late private reads and unmount aborts the replacement reads", async () => {
  const oldSettings = held(settings([reference(alice)], 8));
  const oldConnections = held(connections([alice]));
  const newSettings = held(settings([builtin], 3));
  const newConnections = held(connections());
  vi.mocked(metadataPreferencesApi.settings).mockImplementation((account) =>
    account === "alice" ? oldSettings.promise : newSettings.promise,
  );
  vi.mocked(pluginsApi.settings).mockImplementation((account) =>
    account === "alice" ? oldConnections.promise : newConnections.promise,
  );
  const view = render(preferences());
  try {
    await waitFor(() =>
      expect(metadataPreferencesApi.settings).toHaveBeenCalledTimes(1),
    );
    const oldRead = vi.mocked(metadataPreferencesApi.settings).mock.calls[0][1];
    const oldPlugins = vi.mocked(pluginsApi.settings).mock.calls[0][1];
    view.rerender(preferences("bob"));
    expect(oldRead.aborted && oldPlugins.aborted).toBe(true);
    expect(screen.queryByText(alice.service.name, { exact: true })).toBeNull();
    await waitFor(() =>
      expect(metadataPreferencesApi.settings).toHaveBeenCalledTimes(2),
    );
    await act(async () => {
      await Promise.all([oldSettings.finish(), oldConnections.finish()]);
    });
    expect(
      screen.queryByRole("switch", {
        name: "Use metadata providers",
      }),
    ).toBeNull();
    expect(
      screen.getByText("Loading metadata preferences…", { exact: true }),
    ).toBeTruthy();
    expect(screen.queryByText(alice.service.name, { exact: true })).toBeNull();
    const newRead = vi.mocked(metadataPreferencesApi.settings).mock.calls[1][1];
    const newPlugins = vi.mocked(pluginsApi.settings).mock.calls[1][1];
    view.unmount();
    expect(newRead.aborted && newPlugins.aborted).toBe(true);
    await act(async () => {
      await Promise.all([newSettings.finish(), newConnections.finish()]);
    });
    expect(
      screen.queryByRole("switch", {
        name: "Use metadata providers",
      }),
    ).toBeNull();
    expect(metadataPreferencesApi.update).not.toHaveBeenCalled();
    expect(unexpectedRequest).not.toHaveBeenCalled();
  } finally {
    view.unmount();
    await act(async () => {
      await Promise.all([
        oldSettings.finish(),
        oldConnections.finish(),
        newSettings.finish(),
        newConnections.finish(),
      ]);
    });
  }
});

test("an old account's late save cannot publish success or private state into its replacement", async () => {
  const old = settings([reference(alice)], 8);
  const update = held(settings(old.providers, 9, false));
  vi.mocked(metadataPreferencesApi.settings).mockImplementation((account) =>
    Promise.resolve(account === "alice" ? old : settings([builtin], 4)),
  );
  vi.mocked(pluginsApi.settings).mockImplementation((account) =>
    Promise.resolve(account === "alice" ? connections([alice]) : connections()),
  );
  vi.mocked(metadataPreferencesApi.update).mockReturnValue(update.promise);
  const view = render(preferences());
  try {
    await screen.findByText(alice.service.name, { exact: true });
    fireEvent.click(enabled());
    fireEvent.click(save());
    await waitFor(() =>
      expect(metadataPreferencesApi.update).toHaveBeenCalledTimes(1),
    );
    const oldSignal = vi.mocked(metadataPreferencesApi.update).mock.calls[0][2];
    view.rerender(preferences("bob"));
    expect(oldSignal.aborted).toBe(true);
    expect(screen.queryByText(alice.service.name, { exact: true })).toBeNull();
    await screen.findByText("MusicBrainz", { exact: true });
    await act(async () => {
      await update.finish();
    });
    expect(isOn(enabled())).toBe(true);
    expect(
      screen.queryByText("Metadata preferences saved.", { exact: true }),
    ).toBeNull();
    expect(screen.queryByText(alice.service.name, { exact: true })).toBeNull();
    expect(
      within(selected()).getByText("MusicBrainz", { exact: true }),
    ).toBeTruthy();
    expect(metadataPreferencesApi.update).toHaveBeenCalledTimes(1);
    expect(unexpectedRequest).not.toHaveBeenCalled();
  } finally {
    view.unmount();
    await act(async () => {
      await update.finish();
    });
  }
});

test("a revision conflict retains the draft until an explicit reload refreshes the accepted revision", async () => {
  const initial = settings([reference(alice)], 3);
  vi.mocked(metadataPreferencesApi.settings).mockResolvedValue(initial);
  vi.mocked(metadataPreferencesApi.update).mockRejectedValueOnce(
    new HttpError(409, "Metadata settings changed. Review them again."),
  );
  const view = render(preferences());
  try {
    await screen.findByText(alice.service.name, { exact: true });
    fireEvent.click(enabled());
    fireEvent.click(save());
    expect((await screen.findByRole("alert")).textContent).toContain(
      "Metadata settings changed. Review them again.",
    );
    expect(isOn(enabled())).toBe(false);
    expect(
      within(selected()).getByText(alice.service.name, { exact: true }),
    ).toBeTruthy();
    expect(metadataPreferencesApi.update).toHaveBeenCalledExactlyOnceWith(
      "alice",
      { revision: 3, enabled: false, providers: initial.providers },
      expect.any(AbortSignal),
    );
    expect(metadataPreferencesApi.settings).toHaveBeenCalledTimes(1);
    vi.mocked(metadataPreferencesApi.settings).mockResolvedValue(
      settings(initial.providers, 4),
    );
    vi.mocked(metadataPreferencesApi.update).mockResolvedValue(
      settings(initial.providers, 5, false),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "Reload metadata preferences",
      }),
    );
    await waitFor(() =>
      expect(metadataPreferencesApi.settings).toHaveBeenCalledTimes(2),
    );
    await screen.findByText(alice.service.name, { exact: true });
    expect(isOn(enabled())).toBe(true);
    expect(metadataPreferencesApi.update).toHaveBeenCalledTimes(1);
    fireEvent.click(enabled());
    fireEvent.click(save());
    await screen.findByText("Metadata preferences saved.", { exact: true });
    expect(metadataPreferencesApi.update).toHaveBeenLastCalledWith(
      "alice",
      { revision: 4, enabled: false, providers: initial.providers },
      expect.any(AbortSignal),
    );
    expect(metadataPreferencesApi.update).toHaveBeenCalledTimes(2);
    expect(unexpectedRequest).not.toHaveBeenCalled();
  } finally {
    view.unmount();
  }
});
