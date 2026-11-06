import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { act, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import {
  attachedPage,
  savedRecording,
} from "../../sources/fixtures/sourceAttachment";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function showDownload(download: Partial<Schema<"DownloadStatus">>) {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const path = `/api/v1/recordings/${savedRecording.id}/download`;
  const sourceChoice = `/api/v1/recordings/${savedRecording.id}/source-choice`;
  const status: Schema<"DownloadStatus"> = {
    status: "queued",
    intent_id: "5b1f2f55-7d1c-4a39-8d55-3a1f0bd7a002",
    recording_id: savedRecording.id,
    source_id: null,
    host_device_id: "owned-download-host",
    choice_revision: 0,
    matches_source_choice: true,
    bytes_received: 0,
    total_bytes: null,
    error: null,
    ...download,
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, options?: RequestInit) => {
      if (url === sourceChoice && options?.method === "GET")
        return Response.json({ error: "Not found" }, { status: 404 });
      if (url === path && options?.method === "GET")
        return Response.json(status);
      throw new Error("A download status view must not write or play audio");
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
}

test("a download retrying after a provider failure says the music service did not deliver the audio", async () => {
  await showDownload({ status: "queued", error: "provider_unavailable" });
  expect(
    await screen.findByText(
      "The music service did not deliver the audio. Hito will try again.",
    ),
  ).toBeTruthy();
  expect(screen.queryByText("Download queued.")).toBeNull();
});

test("a download that failed after its retries gives the reason and the way forward", async () => {
  await showDownload({ status: "failed", error: "transfer_failed" });
  expect(
    await screen.findByText(
      "Download failed because the music service did not deliver the audio. Remove it before requesting another copy.",
    ),
  ).toBeTruthy();
});
