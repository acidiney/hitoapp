// @vitest-environment jsdom
import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ArtworkProvider } from "./ArtworkProvider";
import { NoticeProvider } from "./components/Notices";
import type { components } from "./generated";
import {
  lyricObservation,
  lyricRecordingPage,
  readyLyrics,
} from "./fixtures/recordingLyrics";
import { automaticChoice } from "./fixtures/sourceChoice";
import { log } from "./log";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import { PlaybackProvider } from "./player/PlaybackProvider";

type Schema = components["schemas"];

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  localStorage.clear();
  sessionStorage.clear();
});

test("a stale chosen lyric provider refreshes current authority without losing the choice or fetching lyrics until another explicit Show", async () => {
  const consoleLogs = (["debug", "info", "log", "warn"] as const).map((level) =>
    vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const errors = vi.spyOn(console, "error");
  const debug = vi.spyOn(log, "debug");
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const page = structuredClone(lyricRecordingPage);
  const before = structuredClone(page);
  const { user, detail } = page;
  const base = `/api/v1/recordings/${detail.recording_id}`;
  const attached: Schema["RecordingLyricsProvider"] = {
    observation_id: "8d49c593-d800-4c6d-b398-727d2f0f809f",
    installation_id: "cf7bac50-aaf2-42ed-9070-70c67531c91c",
    installation_revision: 17,
    provider_recording: {
      id: 'attached:lyrics/private?edition="3"',
      revision: 'opaque:"attached-version"',
    },
    label: "Attached lyric provider",
  };
  const primary: Schema["RecordingLyricsProvider"] = {
    observation_id: lyricObservation.id,
    installation_id: lyricObservation.plugin_installation_id,
    installation_revision: 9,
    provider_recording: {
      id: lyricObservation.provider_recording_id,
      revision: lyricObservation.provider_revision,
    },
    label: "Primary metadata provider",
  };
  const result = {
    ...readyLyrics,
    observation_id: attached.observation_id,
    document: {
      ...readyLyrics.document,
      recording: attached.provider_recording,
    },
  } satisfies Schema["LyricsLookup"];
  const requests: { path: string; method: string; options?: RequestInit }[] =
    [];
  const responses: Response[] = [];
  const reply = (value: unknown, status = 200) => {
    const response = Response.json(value, { status });
    responses.push(response);
    return response;
  };
  const privateError = 'Private connection "credential-revision" changed';
  let listReads = 0;
  let documentReads = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      const method = options?.method ?? "GET";
      requests.push({ path, method, options });
      if (method === "GET" && path === `${base}/download`)
        return reply({ error: "Not found" }, 404);
      if (method === "GET" && path === `${base}/source-choice`)
        return reply(automaticChoice);
      if (method === "GET" && path === `${base}/lyrics/providers`) {
        listReads++;
        return reply({
          recording_id: detail.recording_id,
          items: [
            primary,
            { ...attached, installation_revision: listReads === 1 ? 17 : 18 },
          ],
        } satisfies Schema["RecordingLyricsProviders"]);
      }
      if (method === "POST" && path === `${base}/lyrics`) {
        documentReads++;
        return documentReads === 1
          ? reply({ error: privateError }, 409)
          : reply(result);
      }
      throw new Error(
        "Unexpected request outside owned lyric authority recovery",
      );
    }),
  );
  const content = () =>
    createElement(
      NoticeProvider,
      null,
      createElement(ArtworkProvider, {
        account: user.id,
        children: createElement(PlaybackProvider, {
          account: user.id,
          children: createElement(RecordingDetailsPage, page),
        }),
      }),
    );
  const view = render(content());
  try {
    await waitFor(() => expect(requests).toHaveLength(2));
    const editor = within(
      screen.getByRole("region", { name: "Your track details" }),
    );
    const title = editor.getByRole<HTMLInputElement>("textbox", {
      name: "Track title",
    });
    const draft = 'Unsent private "authority-recovery" edit';
    fireEvent.change(title, { target: { value: draft } });
    const lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
    fireEvent.click(
      lyrics.getByRole("button", { name: "Choose lyric provider" }),
    );
    const selector = await lyrics.findByRole<HTMLSelectElement>("combobox", {
      name: "Lyric provider",
    });
    fireEvent.change(selector, { target: { value: attached.observation_id } });
    expect(documentReads).toBe(0);
    fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    await lyrics.findByRole("alert");
    expect(documentReads).toBe(1);
    expect(lyrics.queryByText(privateError)).toBeNull();
    expect(lyrics.queryByRole("list", { name: "Lyric lines" })).toBeNull();
    fireEvent.click(
      lyrics.getByRole("button", {
        name: /^Refresh (track details|lyric providers)$/,
      }),
    );
    view.rerender(content());
    await waitFor(() => expect(listReads).toBe(2));
    const refreshed = await lyrics.findByRole<HTMLSelectElement>("combobox", {
      name: "Lyric provider",
    });
    expect(refreshed.value).toBe(attached.observation_id);
    expect(documentReads).toBe(1);
    expect(lyrics.queryByRole("list", { name: "Lyric lines" })).toBeNull();
    expect(lyrics.queryByRole("alert")).toBeNull();
    expect(title.value).toBe(draft);
    expect(editor.getByRole("textbox", { name: "Track title" })).toBe(title);
    expect(page).toEqual(before);
    fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    const lines = await lyrics.findByRole("list", { name: "Lyric lines" });
    expect(
      within(lines)
        .getAllByRole("listitem")
        .map((line) => line.textContent),
    ).toEqual(result.document.lyrics.lines.map((line) => line.text));
    const writes = requests.filter((request) => request.method !== "GET");
    expect(writes.map((request) => request.path)).toEqual([
      `${base}/lyrics`,
      `${base}/lyrics`,
    ]);
    expect(
      writes.map((request) => JSON.parse(String(request.options?.body))),
    ).toEqual(
      [17, 18].map(
        (installation_revision) =>
          ({
            observation_id: attached.observation_id,
            installation_revision,
          }) satisfies Schema["ReadRecordingLyrics"],
      ),
    );
    expect(listReads).toBe(2);
    expect(documentReads).toBe(2);
    expect(requests).toHaveLength(6);
    for (const request of requests) {
      expect(new Headers(request.options?.headers).get("x-hito-account")).toBe(
        user.id,
      );
      expect(request.options?.credentials).toBe("same-origin");
      expect(request.options?.signal).toBeInstanceOf(AbortSignal);
    }
    expect(responses.every((response) => response.bodyUsed)).toBe(true);
    expect(title.value).toBe(draft);
    expect(page).toEqual(before);
    expect(play).not.toHaveBeenCalled();
    expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
    expect(router.reload).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    expect(localStorage.length + sessionStorage.length).toBe(0);
    view.unmount();
    expect(requests.every((request) => request.options?.signal?.aborted)).toBe(
      true,
    );
    expect(errors).not.toHaveBeenCalled();
    const events = debug.mock.calls.filter(([event]) =>
      event.startsWith("lyrics."),
    );
    const lookups = events.filter(([event]) => event === "lyrics.finished");
    const lists = events.filter(
      ([event]) => event === "lyrics.providers.finished",
    );
    expect(lookups.map(([, fields]) => fields?.reason)).toEqual([
      "failed",
      "ready",
    ]);
    expect(lists.map(([, fields]) => fields?.reason)).toEqual(["read", "read"]);
    for (const [, fields] of [...lookups, ...lists])
      expect(fields?.request_id).toMatch(
        /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/,
      );
    const output = JSON.stringify([
      ...consoleLogs.flatMap((entry) => entry.mock.calls),
      ...events,
    ]);
    expect(output.length).toBeLessThanOrEqual(16384);
    for (const value of [
      draft,
      detail.title.value,
      privateError,
      attached.label,
      attached.provider_recording.id,
      attached.provider_recording.revision!,
      readyLyrics.document.lyrics.id,
      readyLyrics.document.lyrics.revision,
      ...readyLyrics.document.lyrics.lines.map((line) => line.text),
    ]) {
      const encoded = JSON.stringify(value).slice(1, -1);
      expect(
        output.includes(value) || output.includes(encoded),
        "Lyric authority recovery logs must exclude every private canary",
      ).toBe(false);
    }
  } finally {
    view.unmount();
  }
});
