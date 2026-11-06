import type { Schema } from "../../../shared/schema";
import { isOn } from "../../../test/switch";
import { createElement } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import SourcePreferences from "../components/SourcePreferences";
import { sourcePreferenceApi } from "../api/sourcePreferenceApi";
import { pluginsApi } from "../../plugins/api/pluginsApi";

vi.mock("../api/sourcePreferenceApi", () => ({
  sourcePreferenceApi: { settings: vi.fn(), update: vi.fn(), forget: vi.fn() },
}));
vi.mock("../../plugins/api/pluginsApi", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../plugins/api/pluginsApi")>();
  return { pluginsApi: { ...actual.pluginsApi, settings: vi.fn() } };
});

const jamendo = "20000000-0000-4000-8000-000000000001";
const youtube = "20000000-0000-4000-8000-000000000002";
const initial: Schema<"SourcePreferences"> = {
  revision: 0,
  fallback: true,
  entries: [
    { kind: "local_files" },
    { kind: "saved_copies" },
    { kind: "provider", installation_id: jamendo },
    { kind: "provider", installation_id: youtube },
  ],
};
const connections = {
  credential_storage_ready: true,
  services: [
    { id: "s1", name: "Jamendo", base_url: "https://j.invalid/", revision: 1 },
    { id: "s2", name: "YouTube", base_url: "https://y.invalid/", revision: 1 },
  ],
  installations: [
    {
      id: jamendo,
      service_id: "s1",
      revision: 1,
      checked_at: 1,
      credential_set: false,
      manifest: {
        id: "a",
        name: "Jamendo",
        protocol_version: 1,
        capabilities: ["sources"],
      },
    },
    {
      id: youtube,
      service_id: "s2",
      revision: 1,
      checked_at: 1,
      credential_set: false,
      manifest: {
        id: "b",
        name: "YouTube",
        protocol_version: 1,
        capabilities: ["sources"],
      },
    },
  ],
} as unknown as Schema<"PluginSettings">;

function order() {
  const list = screen.getByRole("list", { name: "Source order" });
  return within(list)
    .getAllByRole("heading", { level: 4 })
    .map((heading) => heading.textContent);
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(sourcePreferenceApi.settings).mockResolvedValue(initial);
  vi.mocked(pluginsApi.settings).mockResolvedValue(connections);
});

test("lists local files, saved copies and each connected provider by name in priority order", async () => {
  render(createElement(SourcePreferences, { account: "alice" }));
  await screen.findByRole("list", { name: "Source order" });
  expect(order()).toEqual([
    "Local files",
    "Saved copies",
    "Jamendo",
    "YouTube",
  ]);
  expect(screen.getByText("Priority 3")).toBeTruthy();
  expect(
    (
      screen.getByRole("button", {
        name: "Move up Local files",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  expect(
    (
      screen.getByRole("button", {
        name: "Move down YouTube",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  expect(
    isOn(
      screen.getByRole("switch", { name: "Try the next source if one fails" }),
    ),
  ).toBe(true);
  expect(sourcePreferenceApi.update).not.toHaveBeenCalled();
});

test("moving a source changes only the draft until Save sends the exact order and revision", async () => {
  const saved: Schema<"SourcePreferences"> = {
    revision: 1,
    fallback: true,
    entries: [
      initial.entries[0],
      initial.entries[1],
      initial.entries[3],
      initial.entries[2],
    ],
  };
  vi.mocked(sourcePreferenceApi.update).mockResolvedValue(saved);
  render(createElement(SourcePreferences, { account: "alice" }));
  await screen.findByRole("list", { name: "Source order" });
  const save = screen.getByRole("button", {
    name: "Save source preference",
  }) as HTMLButtonElement;
  expect(save.disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Move up YouTube" }));
  expect(order()).toEqual([
    "Local files",
    "Saved copies",
    "YouTube",
    "Jamendo",
  ]);
  expect(sourcePreferenceApi.update).not.toHaveBeenCalled();
  expect(save.disabled).toBe(false);
  fireEvent.click(save);
  await waitFor(() =>
    expect(sourcePreferenceApi.update).toHaveBeenCalledExactlyOnceWith(
      "alice",
      { revision: 0, fallback: true, entries: saved.entries },
      expect.any(AbortSignal),
    ),
  );
  await screen.findByText("Source preference saved.");
  expect(order()).toEqual([
    "Local files",
    "Saved copies",
    "YouTube",
    "Jamendo",
  ]);
  expect(save.disabled).toBe(true);
});

test("the fallback switch is saved with the order and a conflict keeps the draft until reload", async () => {
  vi.mocked(sourcePreferenceApi.update).mockRejectedValueOnce(
    new Error("Source preferences changed. Review them again."),
  );
  render(createElement(SourcePreferences, { account: "alice" }));
  const fallback = await screen.findByRole("switch", {
    name: "Try the next source if one fails",
  });
  fireEvent.click(fallback);
  expect(isOn(fallback)).toBe(false);
  fireEvent.click(
    screen.getByRole("button", { name: "Save source preference" }),
  );
  await screen.findByText("Source preferences changed. Review them again.");
  expect(sourcePreferenceApi.update).toHaveBeenCalledExactlyOnceWith(
    "alice",
    { revision: 0, fallback: false, entries: initial.entries },
    expect.any(AbortSignal),
  );
  expect(isOn(fallback)).toBe(false);
  vi.mocked(sourcePreferenceApi.settings).mockResolvedValue({
    ...initial,
    revision: 3,
  });
  fireEvent.click(
    screen.getByRole("button", { name: "Reload source preference" }),
  );
  await waitFor(() =>
    expect(sourcePreferenceApi.settings).toHaveBeenCalledTimes(2),
  );
  await waitFor(() =>
    expect(
      isOn(
        screen.getByRole("switch", {
          name: "Try the next source if one fails",
        }),
      ),
    ).toBe(true),
  );
  expect(sourcePreferenceApi.forget).toHaveBeenCalledWith("alice");
});

test("another account does not see the previous account's draft", async () => {
  const view = render(createElement(SourcePreferences, { account: "alice" }));
  await screen.findByRole("list", { name: "Source order" });
  fireEvent.click(screen.getByRole("button", { name: "Move up YouTube" }));
  vi.mocked(sourcePreferenceApi.settings).mockResolvedValue({
    revision: 0,
    fallback: true,
    entries: [{ kind: "local_files" }, { kind: "saved_copies" }],
  });
  view.rerender(createElement(SourcePreferences, { account: "bob" }));
  await waitFor(() => expect(order()).toEqual(["Local files", "Saved copies"]));
  expect(sourcePreferenceApi.settings).toHaveBeenLastCalledWith(
    "bob",
    expect.any(AbortSignal),
  );
});
