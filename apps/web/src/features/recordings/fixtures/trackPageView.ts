import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { vi } from "vitest";
import { render } from "@testing-library/react";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import RecordingDetailsPage from "../pages/RecordingDetailsPage";

export type TrackView = {
  choice?: Schema<"SourceChoice"> | null;
  search?: string;
  hash?: string;
  extra?: (path: string, options?: RequestInit) => Response | undefined;
};

export function renderTrackPage(
  page: Schema<"RecordingDetailsPageProps">,
  { choice = null, search = "", hash = "", extra }: TrackView = {},
) {
  window.history.replaceState(
    null,
    "",
    `/recordings/${page.detail.recording_id}${search}${hash}`,
  );
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const base = `/api/v1/recordings/${page.detail.recording_id}`;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const handled = extra?.(path, options);
    if (handled) return handled;
    if (path === `${base}/lyrics/identities`)
      return Response.json({
        recording_id: page.detail.recording_id,
        items: [],
      } satisfies Schema<"RecordingLyricIdentities">);
    if (path === `${base}/lyrics/providers`)
      return Response.json({
        recording_id: page.detail.recording_id,
        items: [],
      } satisfies Schema<"RecordingLyricsProviders">);
    if (path === `${base}/lyrics/services`)
      return Response.json({
        recording_id: page.detail.recording_id,
        items: [],
      } satisfies Schema<"RecordingLyricServices">);
    if (path === `${base}/download`)
      return Response.json({ error: "Not found" }, { status: 404 });
    if (path === `${base}/source-choice`)
      return choice
        ? Response.json(choice)
        : Response.json({
            recording_id: page.detail.recording_id,
            revision: 0,
            mode: "automatic",
            sources: [],
          } satisfies Schema<"SourceChoice">);
    throw new Error(`Unexpected request ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(ArtworkProvider, {
        account: page.user.id,
        children: createElement(PlaybackProvider, {
          account: page.user.id,
          children: createElement(RecordingDetailsPage, page),
        }),
      }),
    ),
  );
  return { ...view, fetch, play };
}
