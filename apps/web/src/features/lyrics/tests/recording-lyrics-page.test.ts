import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import type { Schema } from "../../../shared/schema";
import { log } from "../../../shared/log";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import {
  lyricObservation,
  lyricRecordingPage,
  lyricSettings,
  readyLyrics,
} from "../fixtures/recordingLyrics";
import { automaticChoice } from "../../sources/fixtures/sourceChoice";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("track details reads exact provider lyrics on request without changing personal details or preparing audio", async () => {
  const consoleLogs = (["debug", "info", "log", "warn", "error"] as const).map(
    (level) => vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const debug = vi.spyOn(log, "debug");
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const before = structuredClone(lyricRecordingPage);
  const { user, detail } = lyricRecordingPage;
  const base = `/api/v1/recordings/${detail.recording_id}`;
  const responses: Response[] = [];
  const reply = (value: unknown, status = 200) => {
    const response = Response.json(value, { status });
    responses.push(response);
    return response;
  };
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/download`)
      return reply({ error: "Not found" }, 404);
    if (method === "GET" && path === `${base}/source-choice`)
      return reply(automaticChoice);
    if (method === "GET" && path === "/api/v1/plugins")
      return reply(lyricSettings);
    if (method === "POST" && path === `${base}/lyrics`)
      return reply(readyLyrics);
    throw new Error(
      "Unexpected HTTP request outside the owned lyric read workflow",
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
    await waitFor(() =>
      expect(fetch.mock.calls.map(([path]) => path).sort()).toEqual(
        [`${base}/download`, `${base}/source-choice`].sort(),
      ),
    );
    const title = within(
      screen.getByRole("region", { name: "Your track details" }),
    ).getByRole<HTMLInputElement>("textbox", { name: "Track title" });
    const draft = 'Owned unsent "private" title';
    fireEvent.change(title, { target: { value: draft } });
    const show = screen.queryByRole("button", { name: "Show lyrics" });
    expect(
      show,
      "Recording details must offer an explicit lyric read before any provider lookup",
    ).not.toBeNull();
    fireEvent.click(show!);
    const lyrics = within(
      await screen.findByRole("region", { name: "Lyrics" }),
    );
    const lines = within(
      await lyrics.findByRole("list", { name: "Lyric lines" }),
    ).getAllByRole("listitem");
    expect(lines.map((line) => line.textContent)).toEqual(
      readyLyrics.document.lyrics.lines.map((line) => line.text),
    );
    expect(lines.every((line) => line.querySelector("strong") === null)).toBe(
      true,
    );
    const credits = lyrics.getByRole("link", {
      name: readyLyrics.document.lyrics.attribution.provider_label,
    });
    expect(credits.getAttribute("href")).toBe(
      readyLyrics.document.lyrics.attribution.content_url,
    );
    expect(credits.getAttribute("rel")?.split(" ").sort()).toEqual([
      "noopener",
      "noreferrer",
    ]);
    expect(credits.getAttribute("referrerpolicy")).toBe("no-referrer");
    expect(
      within(lyrics.getByRole("list", { name: "Creators" }))
        .getAllByRole("listitem")
        .map((line) => line.textContent),
    ).toEqual(readyLyrics.document.lyrics.attribution.creators);
    expect(lyrics.queryAllByRole("slider")).toHaveLength(0);
    expect(fetch).toHaveBeenCalledTimes(4);
    expect(fetch.mock.calls.slice(2).map(([path]) => path)).toEqual([
      "/api/v1/plugins",
      `${base}/lyrics`,
    ]);
    const writes = fetch.mock.calls.filter(
      ([, options]) => options?.method === "POST",
    );
    expect(writes).toHaveLength(1);
    expect(JSON.parse(writes[0][1]!.body as string)).toEqual({
      observation_id: lyricObservation.id,
      installation_revision: 9,
    } satisfies Schema<"ReadRecordingLyrics">);
    for (const [, options] of fetch.mock.calls) {
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(user.id);
      expect(options?.credentials).toBe("same-origin");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
    }
    expect(responses.every((response) => response.bodyUsed)).toBe(true);
    expect(title.value).toBe(draft);
    expect(
      within(
        screen.getByRole("region", { name: "Your track details" }),
      ).getByRole("textbox", { name: "Track title" }),
    ).toBe(title);
    expect(lyricRecordingPage).toEqual(before);
    expect(play).not.toHaveBeenCalled();
    expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
    expect(router.reload).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    expect(localStorage.length + sessionStorage.length).toBe(0);
    const events = debug.mock.calls.filter(([event]) =>
      event.startsWith("lyrics."),
    );
    expect(events.map(([event]) => event)).toEqual([
      "lyrics.started",
      "lyrics.finished",
    ]);
    expect(events[1][1]).toMatchObject({ action: "read", reason: "ready" });
    expect(events[0][1]?.request_id).toMatch(
      /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/,
    );
    expect(events[1][1]?.request_id).toBe(events[0][1]?.request_id);
    const output = JSON.stringify([
      ...consoleLogs.flatMap((entry) => entry.mock.calls),
      ...events,
    ]);
    expect(output.length).toBeLessThanOrEqual(8192);
    for (const privateValue of [
      draft,
      detail.title.value,
      lyricObservation.provider_recording_id,
      lyricObservation.provider_revision,
      lyricSettings.services[0].base_url,
      readyLyrics.document.lyrics.id,
      readyLyrics.document.lyrics.revision,
      ...readyLyrics.document.lyrics.lines.map((line) => line.text),
      readyLyrics.document.lyrics.attribution.content_url,
    ]) {
      const encoded = JSON.stringify(privateValue).slice(1, -1);
      expect(
        output.includes(privateValue) || output.includes(encoded),
        "Lyric workflow logs must exclude every owned private canary",
      ).toBe(false);
    }
    view.unmount();
    expect(
      fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
    ).toBe(true);
    expect(document.querySelectorAll("audio")).toHaveLength(0);
    expect(vi.mocked(console.warn)).not.toHaveBeenCalled();
    expect(vi.mocked(console.error)).not.toHaveBeenCalled();
  } finally {
    view.unmount();
  }
});
