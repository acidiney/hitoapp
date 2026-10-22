import { createElement } from "react";
import { expect, vi } from "vitest";
import { render, waitFor } from "@testing-library/react";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import RecordingDetailsPage from "../pages/RecordingDetailsPage";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import type {
  RecordingDetailsPageProps,
  SourceChoice,
} from "../../../shared/types";

type HistoryRead = {
  account: string;
  path: string;
  response: Promise<Response>;
};

export function recordingAttributionView(
  pages: RecordingDetailsPageProps[],
  history?: HistoryRead,
) {
  const logs = (["debug", "info", "log", "warn", "error"] as const).map(
    (level) => vi.spyOn(console, level).mockImplementation(() => {}),
  );
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const account = new Headers(options?.headers).get("x-hito-account");
    if (options?.method !== "GET")
      throw new Error("The owned attribution view accepts only existing reads");
    if (history && account === history.account && path === history.path)
      return history.response;
    for (const page of pages) {
      if (account !== page.user.id) continue;
      const base = `/api/v1/recordings/${page.detail.recording_id}`;
      if (path === `${base}/download`)
        return Response.json({ error: "Not found" }, { status: 404 });
      if (path === `${base}/source-choice`)
        return Response.json({
          recording_id: page.detail.recording_id,
          revision: 0,
          mode: "automatic",
          sources: [],
        } satisfies SourceChoice);
    }
    throw new Error("Unexpected request outside the owned attribution view");
  });
  vi.stubGlobal("fetch", fetch);
  const tree = (page: RecordingDetailsPageProps) =>
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
    );
  const view = render(tree(pages[0]));
  const requests = () =>
    fetch.mock.calls.map(([path, options]) => [
      new Headers(options?.headers).get("x-hito-account"),
      path,
      options?.method,
    ]);
  const expectedReads = (count: number) =>
    pages.slice(0, count).flatMap((page) => {
      const base = `/api/v1/recordings/${page.detail.recording_id}`;
      return [
        [page.user.id, `${base}/download`, "GET"],
        [page.user.id, `${base}/source-choice`, "GET"],
      ];
    });
  return {
    ...view,
    fetch,
    replace: (page: RecordingDetailsPageProps) => view.rerender(tree(page)),
    async settled(count: number, includeHistory = false) {
      const expected = expectedReads(count);
      if (includeHistory && history)
        expected.push([history.account, history.path, "GET"]);
      await waitFor(() => expect(requests().sort()).toEqual(expected.sort()));
    },
    finish(extraPrivate: string[] = []) {
      const expected = expectedReads(pages.length);
      if (history) expected.push([history.account, history.path, "GET"]);
      expect(requests().sort()).toEqual(expected.sort());
      expect(play).not.toHaveBeenCalled();
      expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
      view.unmount();
      expect(vi.mocked(console.warn).mock.calls.length).toBe(0);
      expect(vi.mocked(console.error).mock.calls.length).toBe(0);
      const output = JSON.stringify(logs.flatMap((log) => log.mock.calls));
      expect(output.length).toBeLessThanOrEqual(2048);
      const privateValues = pages.flatMap(({ detail }) => [
        detail.title.value,
        detail.title.personal,
        detail.title.inherited,
        detail.artist.value,
        detail.artist.personal,
        detail.artist.inherited,
        ...[detail.selected_observation, ...detail.observations.items].flatMap(
          (observation) =>
            observation
              ? [
                  observation.title,
                  observation.artist,
                  ...(observation.attribution
                    ? [
                        ...observation.attribution.creators,
                        observation.attribution.provider_label,
                        observation.attribution.content_url,
                      ]
                    : []),
                ]
              : [],
        ),
      ]);
      for (const value of [...privateValues, ...extraPrivate])
        if (value !== null) expect(output.includes(value)).toBe(false);
    },
  };
}
