import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { choose } from "../../../test/listbox";
import { isOn } from "../../../test/switch";
import { beforeEach, expect, test, vi } from "vitest";
import { metadataPreferencesApi } from "../api/metadataPreferencesApi";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import {
  builtin,
  catalog,
  connections,
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
const unexpectedRequest = vi.fn<typeof fetch>();
beforeEach(() => {
  vi.resetAllMocks();
  unexpectedRequest.mockRejectedValue(
    new Error("Metadata preferences must not contact provider operations"),
  );
  vi.stubGlobal("fetch", unexpectedRequest);
});
function selectedRows() {
  return within(
    screen.getByRole("list", {
      name: "Selected metadata providers",
    }),
  ).getAllByRole("listitem");
}

function add(position: number) {
  fireEvent.click(screen.getByRole("button", { name: "Add provider" }));
  const dialog = screen.getByRole("dialog", {
    name: "Add a metadata provider",
  });
  choose(
    within(dialog).getByRole("combobox", { name: "Metadata provider" }),
    position,
  );
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Add metadata provider" }),
  );
}

function remove(row: HTMLElement) {
  fireEvent.click(within(row).getByRole("button", { name: /^Move / }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Remove" }));
  fireEvent.click(
    within(screen.getByRole("dialog", { name: /^Remove .*\?$/ })).getByRole(
      "button",
      { name: "Remove provider" },
    ),
  );
}

test("personally approved catalogs replace the built-in provider and save their exact chosen order", async () => {
  const first = catalog(1, "Shared catalog name");
  const second = catalog(2, "Shared catalog name");
  const search = catalog(3, "Search only service");
  search.installation.manifest.capabilities = ["search", "sources"];
  vi.mocked(metadataPreferencesApi.settings).mockResolvedValue(
    settings([builtin], 7),
  );
  vi.mocked(pluginsApi.settings).mockResolvedValue(
    connections([first, second, search]),
  );
  const ordered = [reference(second), reference(first)];
  vi.mocked(metadataPreferencesApi.update).mockResolvedValue(
    settings(ordered, 8),
  );
  render(preferences());
  fireEvent.click(await screen.findByRole("button", { name: "Add provider" }));
  const selector = screen.getByRole("combobox", { name: "Metadata provider" });
  fireEvent.click(selector);
  expect(screen.getAllByRole("option")).toHaveLength(2);
  fireEvent.keyDown(selector, { key: "Escape" });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  add(0);
  add(0);
  expect(selectedRows()).toHaveLength(3);
  remove(selectedRows()[0]);
  expect(selectedRows()).toHaveLength(2);
  fireEvent.click(
    within(selectedRows()[1]).getByRole("button", { name: /^Move / }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: "Move earlier" }));
  expect(
    selectedRows().map(
      (row) =>
        within(row).getByText("Shared catalog name", { exact: true })
          .textContent,
    ),
  ).toEqual(["Shared catalog name", "Shared catalog name"]);
  expect(metadataPreferencesApi.update).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Save metadata preferences",
    }),
  );
  await screen.findByText("Metadata preferences saved.", { exact: true });
  expect(metadataPreferencesApi.update).toHaveBeenCalledExactlyOnceWith(
    "alice",
    { revision: 7, enabled: true, providers: ordered },
    expect.any(AbortSignal),
  );
  expect(
    within(
      screen.getByRole("list", { name: "Selected metadata providers" }),
    ).queryByText("MusicBrainz", { exact: true }),
  ).toBeNull();
  for (const value of [
    first.installation.id,
    second.installation.id,
    first.service.base_url,
    second.service.base_url,
  ])
    expect(screen.queryByText(value, { exact: true })).toBeNull();
  expect(unexpectedRequest).not.toHaveBeenCalled();
});

test("the selected provider list cannot grow past its account limit", async () => {
  const catalogs = Array.from({ length: 9 }, (_, index) => catalog(index + 1));
  const retained = [builtin, ...catalogs.slice(0, 7).map(reference)];
  vi.mocked(metadataPreferencesApi.settings).mockResolvedValue(
    settings(retained, 4),
  );
  vi.mocked(pluginsApi.settings).mockResolvedValue(connections(catalogs));
  render(preferences());
  const opener = (await screen.findByRole("button", {
    name: "Add provider",
  })) as HTMLButtonElement;
  expect(opener.disabled).toBe(true);
  fireEvent.click(opener);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(selectedRows()).toHaveLength(8);
  expect(metadataPreferencesApi.update).not.toHaveBeenCalled();
  expect(unexpectedRequest).not.toHaveBeenCalled();
});

test("a removed selected connection stays unavailable and can be retained while disabling enrichment", async () => {
  const removed = catalog(1, "Removed private catalog");
  const retained = [reference(removed), builtin];
  vi.mocked(metadataPreferencesApi.settings).mockResolvedValue(
    settings(retained, 11),
  );
  vi.mocked(pluginsApi.settings).mockResolvedValue(connections());
  vi.mocked(metadataPreferencesApi.update).mockResolvedValue(
    settings(retained, 12, false),
  );
  render(preferences());
  await screen.findByText("Unavailable saved provider", { exact: true });
  expect(screen.queryByText(removed.installation.id)).toBeNull();
  fireEvent.click(
    screen.getByRole("switch", {
      name: "Use metadata providers",
    }),
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "Save metadata preferences",
    }),
  );
  await waitFor(() =>
    expect(metadataPreferencesApi.update).toHaveBeenCalledExactlyOnceWith(
      "alice",
      { revision: 11, enabled: false, providers: retained },
      expect.any(AbortSignal),
    ),
  );
  await screen.findByText("Metadata preferences saved.", { exact: true });
  expect(selectedRows()).toHaveLength(2);
  expect(
    screen.getByText("Unavailable saved provider", { exact: true }),
  ).toBeTruthy();
  expect(unexpectedRequest).not.toHaveBeenCalled();
});
