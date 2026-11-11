import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import PluginPlaybackOrder from "../components/PluginPlaybackOrder";
import { sourcePreferenceApi } from "../../sources/api/sourcePreferenceApi";
import { pluginsApi } from "../api/pluginsApi";
import {
  UnsavedContext,
  createUnsavedRegistry,
} from "../../../shared/unsavedChanges";

vi.mock("../../sources/api/sourcePreferenceApi", () => ({
  sourcePreferenceApi: { settings: vi.fn(), update: vi.fn(), forget: vi.fn() },
}));
vi.mock("../api/pluginsApi", () => ({ pluginsApi: { settings: vi.fn() } }));

const youtube = "20000000-0000-4000-8000-000000000002";
const initial: Schema<"SourcePreferences"> = {
  revision: 0,
  fallback: true,
  entries: [
    { kind: "local_files" },
    { kind: "saved_copies" },
    { kind: "provider", installation_id: youtube },
  ],
};

beforeEach(() => {
  vi.mocked(sourcePreferenceApi.settings).mockResolvedValue(initial);
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    credential_storage_ready: true,
    services: [{ id: "svc", name: "YouTube", base_url: "x", revision: 1 }],
    installations: [
      {
        id: youtube,
        service_id: "svc",
        revision: 1,
        checked_at: 1,
        credential_set: true,
        manifest: {
          id: "yt",
          name: "YouTube",
          protocol_version: 1,
          capabilities: ["sources"],
        },
      },
    ],
  });
});

function page() {
  return createElement(
    UnsavedContext.Provider,
    { value: createUnsavedRegistry() },
    createElement(PluginPlaybackOrder, { account: "alice" }),
  );
}

test("the panel lists the sources in order with the plugin by name and a fallback choice", async () => {
  render(page());
  const list = await screen.findByRole("list", { name: "Source order" });
  const names = within(list)
    .getAllByRole("listitem")
    .map((item) => item.querySelector("strong")?.textContent);
  expect(names).toEqual(["Local files", "Saved copies", "YouTube"]);
  expect(
    (
      screen.getByRole("checkbox", {
        name: /Try the next source/,
      }) as HTMLInputElement
    ).checked,
  ).toBe(true);
  expect(
    screen.queryByRole("button", { name: "Save playback order" }),
  ).toBeNull();
});

test("moving a source offers to save and saves the new order", async () => {
  vi.mocked(sourcePreferenceApi.update).mockImplementation(
    async (_account, body) => ({
      ...body,
      revision: 1,
    }),
  );
  render(page());
  await screen.findByRole("list", { name: "Source order" });
  fireEvent.click(screen.getByRole("button", { name: "Move up YouTube" }));
  fireEvent.click(screen.getByRole("button", { name: "Save playback order" }));
  await screen.findByText("Playback order saved.");
  expect(sourcePreferenceApi.update).toHaveBeenCalledWith(
    "alice",
    {
      revision: 0,
      fallback: true,
      entries: [
        { kind: "local_files" },
        { kind: "provider", installation_id: youtube },
        { kind: "saved_copies" },
      ],
    },
    expect.any(AbortSignal),
  );
});
