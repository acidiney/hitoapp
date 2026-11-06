import { createElement, type ReactNode } from "react";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { change } from "../../../shared/sync";
import type { Schema } from "../../../shared/schema";
import {
  lyricRecordingPage,
  lyricSettings,
  readyLyrics,
} from "../../lyrics/fixtures/recordingLyrics";
import { setAutomaticLyrics } from "../../lyrics/fixtures/automaticLyrics";
import { choiceSourceA } from "../../sources/fixtures/sourceChoice";
import { attributedRecordingPage } from "../fixtures/recordingAttribution";
import { renderTrackPage } from "../fixtures/trackPageView";

setAutomaticLyrics(true);

vi.mock("../../../shared/sync", () => ({ change: vi.fn() }));
vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  vi.restoreAllMocks();
  vi.mocked(change).mockReset();
  window.history.replaceState(null, "", "/");
});

function lyricsPage(lyrics: Schema<"LyricsLookup">, search = "") {
  const base = `/api/v1/recordings/${lyricRecordingPage.detail.recording_id}`;
  const posts: string[] = [];
  const view = renderTrackPage(lyricRecordingPage, {
    search,
    extra: (path, options) => {
      if (path === "/api/v1/plugins") return Response.json(lyricSettings);
      if (path === `${base}/lyrics` && options?.method === "POST") {
        posts.push(path);
        return Response.json(lyrics);
      }
      return undefined;
    },
  });
  return { ...view, posts };
}

const synced = (patch: Partial<Schema<"TimedLyricLine">>[]) => ({
  ...readyLyrics,
  document: {
    ...readyLyrics.document,
    lyrics: {
      ...readyLyrics.document.lyrics,
      lines: readyLyrics.document.lyrics.lines.map((line, index) => ({
        ...line,
        ...patch[index],
      })),
    },
  },
});

test("lyrics load on their own for a track without audio and the Overview previews them", async () => {
  const { posts } = lyricsPage(readyLyrics);
  const preview = await screen.findByRole("list", { name: "Lyrics preview" });
  expect(posts).toHaveLength(1);
  expect(within(preview).getAllByRole("listitem")).toHaveLength(2);
  expect(screen.getByRole("button", { name: "All lyrics" })).toBeTruthy();
});

test("the Lyrics tab names synced lyrics Synced", async () => {
  lyricsPage(synced([{ start_ms: 0 }, { start_ms: 1_000 }]), "?tab=lyrics");
  const region = await screen.findByRole("region", { name: "Lyrics" });
  await within(region).findByRole("list", { name: "Lyric lines" });
  expect(within(region).getByText("Synced")).toBeTruthy();
});

test("lyrics without timing are named Unsynced", async () => {
  lyricsPage(synced([{ start_ms: 0 }, { start_ms: 0 }]), "?tab=lyrics");
  const region = await screen.findByRole("region", { name: "Lyrics" });
  await within(region).findByRole("list", { name: "Lyric lines" });
  expect(within(region).getByText("Unsynced")).toBeTruthy();
  expect(within(region).queryByText("Synced")).toBeNull();
});

test("a provider with no lyrics for the track says so and the Overview offers a lookup", async () => {
  lyricsPage({
    ...readyLyrics,
    document: { ...readyLyrics.document, lyrics: null },
  });
  expect(
    await screen.findByRole("button", { name: "Look up lyrics" }),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Look up lyrics" }));
  const region = screen.getByRole("region", { name: "Lyrics" });
  expect(
    await within(region).findByText("No lyrics found for this recording."),
  ).toBeTruthy();
});

test("a failed automatic lookup keeps the error and a retry in the Lyrics tab", async () => {
  const base = `/api/v1/recordings/${lyricRecordingPage.detail.recording_id}`;
  renderTrackPage(lyricRecordingPage, {
    search: "?tab=lyrics",
    extra: (path, options) => {
      if (path === "/api/v1/plugins") return Response.json(lyricSettings);
      if (path === `${base}/lyrics` && options?.method === "POST")
        return Response.json({ error: "down" }, { status: 502 });
      return undefined;
    },
  });
  const region = await screen.findByRole("region", { name: "Lyrics" });
  expect(await within(region).findByRole("alert")).toBeTruthy();
  expect(
    within(region).getByRole("button", { name: "Retry lyrics" }),
  ).toBeTruthy();
});

test("turning automatic lyrics off keeps the Lyrics tab waiting for an explicit request", async () => {
  vi.spyOn(Storage.prototype, "getItem").mockReturnValue("off");
  const { posts } = lyricsPage(readyLyrics, "?tab=lyrics");
  const region = await screen.findByRole("region", { name: "Lyrics" });
  expect(
    within(region).getByRole("button", { name: "Show lyrics" }),
  ).toBeTruthy();
  expect(posts).toHaveLength(0);
});

test("rows without a value are left out instead of showing placeholders", () => {
  const page = structuredClone(attributedRecordingPage);
  page.detail.selected_observation = null;
  page.detail.musicbrainz_recording_id = null;
  renderTrackPage(page);
  const panel = screen.getByRole("tabpanel", { name: "Overview" });
  const labels = [...panel.querySelectorAll("dt")].map((dt) => dt.textContent);
  expect(labels).toEqual(["Title", "Artist", "Source"]);
  expect(panel.textContent).not.toMatch(/Unknown|Never|Format|Last played/);
  expect(within(panel).getByText(/None/)).toBeTruthy();
  expect(within(panel).queryByRole("complementary")).toBeNull();
});

test("pressing Escape in an inline editor closes it without saving and returns focus to the pencil", async () => {
  renderTrackPage(attributedRecordingPage, { search: "?tab=metadata" });
  fireEvent.click(screen.getByRole("button", { name: "Edit track title" }));
  const input = screen.getByRole("textbox", { name: "Track title" });
  await waitFor(() => expect(document.activeElement).toBe(input));
  fireEvent.change(input, { target: { value: "Changed" } });
  fireEvent.keyDown(input, { key: "Escape" });
  expect(screen.queryByRole("textbox", { name: "Track title" })).toBeNull();
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Edit track title" }),
    ),
  );
  expect(change).not.toHaveBeenCalled();
});

test("pressing Enter in an inline editor saves a personal value", async () => {
  vi.mocked(change).mockResolvedValue({ acknowledged: [] } as never);
  renderTrackPage(attributedRecordingPage, { search: "?tab=metadata" });
  fireEvent.click(screen.getByRole("button", { name: "Edit track title" }));
  const input = screen.getByRole("textbox", { name: "Track title" });
  fireEvent.change(input, { target: { value: "Mine" } });
  fireEvent.submit(input.closest("form")!);
  await waitFor(() =>
    expect(change).toHaveBeenCalledWith(
      attributedRecordingPage.user.id,
      {
        kind: "recording_title_set",
        recording_id: attributedRecordingPage.detail.recording_id,
        title: "Mine",
      },
      expect.any(AbortSignal),
    ),
  );
});

test("Reset all to source only appears with personal edits and returns each one to its source", async () => {
  vi.mocked(change).mockResolvedValue({ acknowledged: [] } as never);
  const base = structuredClone(attributedRecordingPage);
  base.detail.title = { ...base.detail.title, personal: null };
  base.detail.artist = { ...base.detail.artist, personal: null };
  const plain = renderTrackPage(base, { search: "?tab=metadata" });
  expect(
    screen.queryByRole("button", { name: "Reset all to source" }),
  ).toBeNull();
  plain.unmount();
  const page = structuredClone(base);
  page.detail.title.personal = "Mine";
  page.detail.title.operation_id = "11111111-1111-4111-8111-111111111111";
  renderTrackPage(page, { search: "?tab=metadata" });
  expect(screen.getByText("1 personal edit")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Reset all to source" }));
  await waitFor(() =>
    expect(change).toHaveBeenCalledWith(
      page.user.id,
      {
        kind: "recording_title_inherit",
        recording_id: page.detail.recording_id,
      },
      expect.any(AbortSignal),
    ),
  );
  expect(change).toHaveBeenCalledTimes(1);
});

test("a local file source hides the server copy and a provider source shows it", async () => {
  const entry = (provider: string): Schema<"SourceChoice"> => ({
    recording_id: attributedRecordingPage.detail.recording_id,
    revision: 1,
    mode: "automatic",
    sources: [
      {
        source: { ...choiceSourceA, provider, label: "A source" },
        selected: true,
        selectable: true,
      },
    ],
  });
  const local = renderTrackPage(attributedRecordingPage, {
    search: "?tab=sources",
    choice: entry("local"),
  });
  await screen.findByRole("region", { name: "Playback source" });
  expect(screen.queryByRole("region", { name: "Server copy" })).toBeNull();
  local.unmount();
  renderTrackPage(attributedRecordingPage, {
    search: "?tab=sources",
    choice: entry("plugin"),
  });
  await screen.findByRole("region", { name: "Playback source" });
  expect(screen.getByRole("region", { name: "Server copy" })).toBeTruthy();
});

test("the history tab lists reads with a category word", () => {
  renderTrackPage(attributedRecordingPage, { search: "?tab=history" });
  const panel = screen.getByRole("tabpanel", { name: "History" });
  expect(within(panel).getAllByText("Read").length).toBeGreaterThan(0);
});
