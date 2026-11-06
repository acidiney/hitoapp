import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { act, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { setAutomaticLyrics } from "../../lyrics/fixtures/automaticLyrics";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import {
  attachedPage,
  savedRecording,
} from "../../sources/fixtures/sourceAttachment";

setAutomaticLyrics(false);

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("a download waiting for disk space says so and is not reported as a failure", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const path = `/api/v1/recordings/${savedRecording.id}/download`;
  const sourceChoice = `/api/v1/recordings/${savedRecording.id}/source-choice`;
  const waiting: Schema<"DownloadStatus"> = {
    status: "queued",
    intent_id: "5b1f2f55-7d1c-4a39-8d55-3a1f0bd7a001",
    recording_id: savedRecording.id,
    source_id: null,
    host_device_id: "owned-download-host",
    choice_revision: 0,
    matches_source_choice: true,
    bytes_received: 0,
    total_bytes: null,
    error: "insufficient_storage",
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, options?: RequestInit) => {
      if (url === sourceChoice && options?.method === "GET")
        return Response.json({ error: "Not found" }, { status: 404 });
      if (url === path && options?.method === "GET")
        return Response.json(waiting);
      throw new Error("A waiting download must not write or play audio");
    }),
  );
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: attachedPage.user.id,
        children: createElement(RecordingDetailsPage, attachedPage),
      }),
    ),
  );
  await act(async () => {});
  expect(
    await screen.findByText(
      "Not enough disk space on this server. Hito will try again.",
    ),
  ).toBeTruthy();
  expect(screen.queryByText("Download failed.", { exact: false })).toBeNull();
  expect(screen.queryByText("Download queued.")).toBeNull();
});
