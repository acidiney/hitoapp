import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { sourceAttachmentPage } from "../fixtures/sourceAttachment";
import type { Source } from "../../../shared/types";

vi.mock("@inertiajs/react", () => ({ Link: "a" }));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({ playQueue: vi.fn() }),
}));
vi.mock("../../downloads/hooks/useRecordingDownload", () => ({
  useRecordingDownload: () => ({}),
}));
vi.mock("../../downloads/components/RecordingDownload", () => ({
  default: () => null,
}));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));
vi.mock("../../artwork/components/ArtworkStatus", () => ({ default: () => null }));
vi.mock("../../metadata/components/MetadataEditor", () => ({ default: () => null }));
vi.mock("../../metadata/components/MetadataObservations", () => ({ default: () => null }));
vi.mock("../components/SourceAttachment", () => ({
  default: () => createElement("p", null, "Provider attachment controls"),
}));

test("local metadata provenance does not hide an eligible plugin audio source", async () => {
  const detail = sourceAttachmentPage.detail;
  const sourceChoice = `/api/v1/recordings/${detail.recording_id}/source-choice`;
  const source: Source = {
    id: "99f1ecbe-613b-4170-a8fe-26f80eb1d6fa",
    recording_id: detail.recording_id,
    provider: "plugin",
    device_id: null,
    label: "Approved remote audio",
    ready: false,
    can_prepare: true,
  };
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    if (url === sourceChoice && options?.method === "GET")
      return Response.json({ error: "Not found" }, { status: 404 });
    if (url.endsWith("/sources/refresh")) return Response.json([source]);
    if (url.endsWith("/preparation") && options?.method === "GET")
      return new Response(null, { status: 404 });
    throw new Error("Unexpected preparation page request");
  });
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(RecordingDetailsPage, {
      ...sourceAttachmentPage,
      detail: {
        ...detail,
        selected_observation: {
          ...detail.selected_observation!,
          origin: "file_inspection",
        },
      },
    }),
  );
  expect(
    screen.getByRole("region", { name: "Audio availability" }),
  ).toBeTruthy();
  expect(screen.queryByText("Provider attachment controls")).toBeNull();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
  });
  expect(screen.getByText(source.label)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Prepare audio" })).toHaveProperty(
    "disabled",
    false,
  );
  const choiceReads = fetch.mock.calls.filter(
    ([url, options]) => url === sourceChoice && options?.method === "GET",
  );
  expect(choiceReads).toHaveLength(1);
  expect(choiceReads[0][1]).toMatchObject({
    credentials: "same-origin",
    headers: { "x-hito-account": sourceAttachmentPage.user.id },
    signal: expect.any(AbortSignal),
  });
  expect(choiceReads[0][1]?.signal?.aborted).toBe(false);
  expect(choiceReads[0][1]?.body).toBeUndefined();
  expect(
    fetch.mock.calls
      .filter(
        ([url, options]) =>
          !(url === sourceChoice && options?.method === "GET"),
      )
      .map(([, options]) => options?.method),
  ).toEqual(["POST", "GET"]);
  expect(
    fetch.mock.calls.some(
      ([url]) => url.includes("/download") || url.includes("/audio"),
    ),
  ).toBe(false);
});
