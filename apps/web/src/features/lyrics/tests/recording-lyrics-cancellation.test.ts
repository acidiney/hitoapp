import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import {
  lyricRecordingPage,
  lyricSettings,
  readyLyrics,
} from "../fixtures/recordingLyrics";
import { automaticChoice } from "../../../fixtures/sourceChoice";
import { log } from "../../../shared/log";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { PlaybackProvider } from "../../../player/PlaybackProvider";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("cancelling held plugin settings prevents its late lyric POST while a fresh attempt succeeds", async () => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  const warnings = vi.spyOn(console, "warn").mockImplementation(() => {});
  const debug = vi.spyOn(log, "debug");
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const before = structuredClone(lyricRecordingPage);
  let release!: (value: Response) => void;
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  const stale = structuredClone(lyricSettings);
  stale.installations[1].revision = 2;
  const oldResponse = Response.json(stale);
  const { user, detail } = lyricRecordingPage;
  const base = `/api/v1/recordings/${detail.recording_id}`;
  let settingsReads = 0;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/download`)
      return Response.json({ error: "Not found" }, { status: 404 });
    if (method === "GET" && path === `${base}/source-choice`)
      return Response.json(automaticChoice);
    if (method === "GET" && path === "/api/v1/plugins")
      return ++settingsReads === 1 ? held : Response.json(lyricSettings);
    if (method === "POST" && path === `${base}/lyrics`)
      return Response.json(readyLyrics);
    throw new Error("Unexpected HTTP request outside owned lyric cancellation");
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
    await waitFor(() => expect(settingsReads).toBe(1));
    expect(lyrics.getByRole("status").textContent).toBe("Loading lyrics…");
    const oldSignal = fetch.mock.calls.find(
      ([path]) => path === "/api/v1/plugins",
    )![1]!.signal!;
    expect(oldSignal.aborted).toBe(false);
    fireEvent.click(
      lyrics.getByRole("button", { name: "Cancel lyric request" }),
    );
    expect(oldSignal.aborted).toBe(true);
    expect(lyrics.queryByRole("status")).toBeNull();
    expect(
      fetch.mock.calls.filter(([, options]) => options?.method === "POST"),
    ).toHaveLength(0);
    fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    await lyrics.findByRole("list", { name: "Lyric lines" });
    expect(settingsReads).toBe(2);
    await act(async () => {
      release(oldResponse);
      await held;
    });
    const writes = fetch.mock.calls.filter(
      ([, options]) => options?.method === "POST",
    );
    expect(writes.map(([path]) => path)).toEqual([`${base}/lyrics`]);
    expect(JSON.parse(writes[0][1]!.body as string)).toEqual({
      observation_id: readyLyrics.observation_id,
      installation_revision: 9,
    });
    expect(fetch).toHaveBeenCalledTimes(5);
    expect(oldResponse.bodyUsed).toBe(true);
    expect(
      within(lyrics.getByRole("list", { name: "Lyric lines" }))
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(readyLyrics.document.lyrics.lines.map((line) => line.text));
    const completed = debug.mock.calls.filter(
      ([event]) => event === "lyrics.finished",
    );
    expect(completed.map(([, fields]) => fields?.reason)).toEqual([
      "ready",
      "cancelled",
    ]);
    expect(completed[0][1]?.request_id).not.toBe(completed[1][1]?.request_id);
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
    await act(async () => {
      release(oldResponse);
      await held;
    });
  }
});
