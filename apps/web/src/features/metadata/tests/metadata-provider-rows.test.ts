import { fireEvent, render, screen, within } from "@testing-library/react";
import { choose } from "../../../test/listbox";
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
beforeEach(() => vi.resetAllMocks());

const archive = catalog(1, "Archive catalog");
const radio = catalog(2, "Radio catalog");

function rows() {
  return within(
    screen.getByRole("list", { name: "Selected metadata providers" }),
  ).getAllByRole("listitem");
}

function names() {
  return rows().map(
    (row) => within(row).getByRole("heading", { level: 3 }).textContent,
  );
}

async function show(
  providers = [builtin, reference(archive), reference(radio)],
) {
  vi.mocked(metadataPreferencesApi.settings).mockResolvedValue(
    settings(providers, 3),
  );
  vi.mocked(pluginsApi.settings).mockResolvedValue(
    connections([archive, radio]),
  );
  render(preferences());
  await screen.findByRole("list", { name: "Selected metadata providers" });
}

test("each row names its provider, kind, status and priority", async () => {
  await show();
  const [first, second] = rows();
  expect(within(first).getByText("MusicBrainz")).toBeTruthy();
  expect(within(first).getByText("Built in")).toBeTruthy();
  expect(within(first).getByText("1")).toBeTruthy();
  expect(within(second).getByText("Archive catalog")).toBeTruthy();
  expect(within(second).getByText("Plugin · connected")).toBeTruthy();
  expect(within(second).getByText("2")).toBeTruthy();
});

test("a saved plugin whose connection is gone is marked unavailable", async () => {
  vi.mocked(metadataPreferencesApi.settings).mockResolvedValue(
    settings([reference(archive)], 3),
  );
  vi.mocked(pluginsApi.settings).mockResolvedValue(connections([]));
  render(preferences());
  const [row] = await screen.findAllByRole("listitem");
  expect(within(row).getByText("Unavailable saved provider")).toBeTruthy();
  expect(within(row).getByText("Plugin · not connected")).toBeTruthy();
});

function moveItem(row: HTMLElement, item: string) {
  fireEvent.click(within(row).getByRole("button", { name: /^Move / }));
  return screen.getByRole("menuitem", { name: item }) as HTMLButtonElement;
}

test("moving a row changes the priority numbers and the end rows cannot move outward", async () => {
  await show();
  const first = moveItem(rows()[0], "Move earlier");
  expect(first.disabled).toBe(true);
  fireEvent.keyDown(first, { key: "Escape" });
  fireEvent.click(moveItem(rows()[0], "Move later"));
  expect(names()).toEqual(["Archive catalog", "MusicBrainz", "Radio catalog"]);
  expect(within(rows()[1]).getByText("2")).toBeTruthy();
  const last = moveItem(rows()[2], "Move later");
  expect(last.disabled).toBe(true);
  fireEvent.keyDown(last, { key: "Escape" });
  fireEvent.click(moveItem(rows()[2], "Move earlier"));
  expect(names()).toEqual(["Archive catalog", "Radio catalog", "MusicBrainz"]);
});

test("the Add provider dialog lists only unused providers, adds the chosen one and returns focus", async () => {
  await show([builtin]);
  const opener = screen.getByRole("button", { name: "Add provider" });
  opener.focus();
  fireEvent.click(opener);
  const dialog = screen.getByRole("dialog", {
    name: "Add a metadata provider",
  });
  const selector = within(dialog).getByRole("combobox", {
    name: "Metadata provider",
  });
  fireEvent.click(selector);
  expect(
    screen.getAllByRole("option").map((option) => option.textContent),
  ).toEqual(["Archive catalog", "Radio catalog"]);
  fireEvent.keyDown(selector, { key: "Escape" });
  choose(selector, "Radio catalog");
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Add metadata provider" }),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(names()).toEqual(["MusicBrainz", "Radio catalog"]);
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Add provider" }),
  );
  expect(metadataPreferencesApi.update).not.toHaveBeenCalled();
});

test("cancelling the Add provider dialog changes nothing", async () => {
  await show([builtin]);
  fireEvent.click(screen.getByRole("button", { name: "Add provider" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(names()).toEqual(["MusicBrainz"]);
});

test("removing a provider asks first and cancelling keeps the row", async () => {
  await show();
  fireEvent.click(moveItem(rows()[1], "Remove"));
  const dialog = screen.getByRole("dialog", {
    name: "Remove Archive catalog?",
  });
  expect(names()).toHaveLength(3);
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(names()).toHaveLength(3);
  fireEvent.click(moveItem(rows()[1], "Remove"));
  fireEvent.click(
    within(
      screen.getByRole("dialog", { name: "Remove Archive catalog?" }),
    ).getByRole("button", { name: "Remove provider" }),
  );
  expect(names()).toEqual(["MusicBrainz", "Radio catalog"]);
  expect(within(rows()[1]).getByText("2")).toBeTruthy();
  expect(metadataPreferencesApi.update).not.toHaveBeenCalled();
});

test("with no provider selected the list says so and Save stays unavailable while enabled", async () => {
  await show([]);
  expect(screen.getByText("No metadata providers selected.")).toBeTruthy();
  expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  expect(
    screen.queryByRole("button", { name: "Save metadata preferences" }),
  ).toBeNull();
});

test("when enrichment is off the rows stay editable and say the order is kept", async () => {
  vi.mocked(metadataPreferencesApi.settings).mockResolvedValue(
    settings([builtin], 3, false),
  );
  vi.mocked(pluginsApi.settings).mockResolvedValue(connections([archive]));
  render(preferences());
  expect(
    await screen.findByText(
      "Metadata providers are off. This order is kept for when you turn them on.",
    ),
  ).toBeTruthy();
  expect(names()).toEqual(["MusicBrainz"]);
});
