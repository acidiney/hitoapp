import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ArtworkProvider } from "./ArtworkProvider";
import { NoticeProvider } from "./components/Notices";
import type { components } from "./generated";
import {
  lyricRecordingPage,
  lyricSettings,
  readyLyrics,
} from "./fixtures/recordingLyrics";
import { automaticChoice } from "./fixtures/sourceChoice";
import { log } from "./log";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import { PlaybackProvider } from "./player/PlaybackProvider";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("null, unsupported and unavailable lyrics stay distinct and each explicit retry reads current connection authority", async () => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  const warnings = vi.spyOn(console, "warn").mockImplementation(() => {});
  const debug = vi.spyOn(log, "debug");
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const before = structuredClone(lyricRecordingPage);
  const results: components["schemas"]["LyricsLookup"][] = [
    { ...readyLyrics, document: { ...readyLyrics.document, lyrics: null } },
    { status: "unsupported" },
    { status: "unavailable" },
    readyLyrics,
  ];
  const { user, detail } = lyricRecordingPage;
  const base = `/api/v1/recordings/${detail.recording_id}`;
  let settingsReads = 0;
  let lyricReads = 0;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/download`)
      return Response.json({ error: "Not found" }, { status: 404 });
    if (method === "GET" && path === `${base}/source-choice`)
      return Response.json(automaticChoice);
    if (method === "GET" && path === "/api/v1/plugins") {
      const settings = structuredClone(lyricSettings);
      settings.installations[1].revision += settingsReads++;
      return Response.json(settings);
    }
    if (method === "POST" && path === `${base}/lyrics`)
      return Response.json(results[lyricReads++]);
    throw new Error(
      "Unexpected HTTP request outside owned lyric result recovery",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(ArtworkProvider, {
        account: user.id,
        children: createElement(PlaybackProvider, {
          account: user.id,
          children: createElement(RecordingDetailsPage, lyricRecordingPage),
        }),
      }),
    ),
  );
  try {
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
    const lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
    fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    await lyrics.findByText("No lyrics found for this recording.");
    expect(lyrics.queryByRole("list", { name: "Lyric lines" })).toBeNull();
    fireEvent.click(lyrics.getByRole("button", { name: "Retry lyrics" }));
    await lyrics.findByText(
      "This provider does not offer lyrics for this track.",
    );
    expect(
      lyrics.queryByText("No lyrics found for this recording."),
    ).toBeNull();
    fireEvent.click(lyrics.getByRole("button", { name: "Retry lyrics" }));
    await lyrics.findByText("Lyrics are unavailable right now. Try again.");
    expect(
      lyrics.queryByText("This provider does not offer lyrics for this track."),
    ).toBeNull();
    fireEvent.click(lyrics.getByRole("button", { name: "Retry lyrics" }));
    await lyrics.findByRole("list", { name: "Lyric lines" });
    expect(
      lyrics.queryByText("Lyrics are unavailable right now. Try again."),
    ).toBeNull();
    expect(settingsReads).toBe(4);
    expect(lyricReads).toBe(4);
    expect(fetch).toHaveBeenCalledTimes(10);
    const commands = fetch.mock.calls.filter(
      ([, options]) => options?.method === "POST",
    );
    expect(commands.map(([path]) => path)).toEqual(
      Array(4).fill(`${base}/lyrics`),
    );
    expect(
      commands.map(([, options]) => JSON.parse(options!.body as string)),
    ).toEqual(
      [9, 10, 11, 12].map((revision) => ({
        observation_id: readyLyrics.observation_id,
        installation_revision: revision,
      })),
    );
    const finished = debug.mock.calls.filter(
      ([event]) => event === "lyrics.finished",
    );
    expect(finished.map(([, fields]) => fields?.reason)).toEqual([
      "not_found",
      "unsupported",
      "unavailable",
      "ready",
    ]);
    expect(new Set(finished.map(([, fields]) => fields?.request_id)).size).toBe(
      4,
    );
    expect(lyricRecordingPage).toEqual(before);
    expect(play).not.toHaveBeenCalled();
    expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
    expect(router.reload).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    expect(localStorage.length + sessionStorage.length).toBe(0);
    view.unmount();
    expect(
      fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
    ).toBe(true);
    expect(warnings).not.toHaveBeenCalled();
    expect(errors).not.toHaveBeenCalled();
  } finally {
    view.unmount();
  }
});
