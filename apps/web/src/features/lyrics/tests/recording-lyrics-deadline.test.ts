import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { setAutomaticLyrics } from "../fixtures/automaticLyrics";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import {
  lyricRecordingPage,
  lyricSettings,
  readyLyrics,
} from "../fixtures/recordingLyrics";
import { automaticChoice } from "../../sources/fixtures/sourceChoice";
import { log } from "../../../shared/log";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";

setAutomaticLyrics(false);

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("a lost lyric reply expires at the owned deadline and retries reject private HTTP errors and late results", async () => {
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
  const expired = structuredClone(readyLyrics);
  expired.document.lyrics.lines[0].text = "Owned expired lyric response";
  const oldResponse = Response.json(expired);
  const privateError = 'Owned private "upstream error" must not be rendered';
  const rejected = Response.json(
    { error: privateError, code: "conflict" },
    { status: 409 },
  );
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
    if (method === "POST" && path === `${base}/lyrics`) {
      lyricReads++;
      return lyricReads === 1
        ? held
        : lyricReads === 2
          ? rejected
          : Response.json(readyLyrics);
    }
    throw new Error(
      "Unexpected HTTP request outside owned lyric deadline recovery",
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
    vi.useFakeTimers();
    const lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
    await act(async () => {
      fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    });
    expect(lyricReads).toBe(1);
    const oldSignal = fetch.mock.calls.find(
      ([path]) => path === `${base}/lyrics`,
    )![1]!.signal!;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(14_999);
    });
    expect(lyrics.getByRole("status").textContent).toBe("Loading lyrics…");
    expect(oldSignal.aborted).toBe(false);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(oldSignal.aborted).toBe(true);
    expect(lyrics.queryByRole("status")).toBeNull();
    expect(lyrics.getByRole("alert").textContent).toBe(
      "Lyrics took too long to load. Try again.",
    );
    await act(async () => {
      fireEvent.click(lyrics.getByRole("button", { name: "Retry lyrics" }));
    });
    expect(lyricReads).toBe(2);
    expect(lyrics.getByRole("alert").textContent).toBe(
      "Track details or the provider connection changed. Refresh this track and try again.",
    );
    expect(lyrics.queryByText(privateError)).toBeNull();
    expect(rejected.bodyUsed).toBe(true);
    expect(router.reload).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(lyrics.getByRole("button", { name: "Retry lyrics" }));
    });
    expect(lyrics.getByRole("list", { name: "Lyric lines" })).toBeTruthy();
    await act(async () => {
      release(oldResponse);
      await held;
    });
    expect(lyrics.queryByText("Owned expired lyric response")).toBeNull();
    expect(
      within(lyrics.getByRole("list", { name: "Lyric lines" }))
        .getAllByRole("listitem")
        .map((line) => line.textContent),
    ).toEqual(readyLyrics.document.lyrics.lines.map((line) => line.text));
    expect(lyrics.queryByRole("alert")).toBeNull();
    expect(fetch).toHaveBeenCalledTimes(8);
    expect(settingsReads).toBe(3);
    const commands = fetch.mock.calls.filter(
      ([, options]) => options?.method === "POST",
    );
    expect(commands.map(([path]) => path)).toEqual(
      Array(3).fill(`${base}/lyrics`),
    );
    expect(
      commands.map(([, options]) => JSON.parse(options!.body as string)),
    ).toEqual(
      [9, 10, 11].map((revision) => ({
        observation_id: readyLyrics.observation_id,
        installation_revision: revision,
      })),
    );
    const finished = debug.mock.calls.filter(
      ([event]) => event === "lyrics.finished",
    );
    expect(finished.map(([, fields]) => fields?.reason)).toEqual([
      "failed",
      "ready",
      "deadline",
    ]);
    expect(new Set(finished.map(([, fields]) => fields?.request_id)).size).toBe(
      3,
    );
    const output = JSON.stringify([...warnings.mock.calls, ...finished]);
    expect(
      output.includes(privateError) ||
        output.includes(JSON.stringify(privateError).slice(1, -1)),
    ).toBe(false);
    expect(warnings).toHaveBeenCalledTimes(1);
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
    expect(errors).not.toHaveBeenCalled();
  } finally {
    view.unmount();
    await act(async () => {
      release(oldResponse);
      await held;
    });
  }
});
