import { createElement, type ReactNode } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AudiobookIndexPage from "../pages/AudiobookIndexPage";
import AudiobookPage from "../pages/AudiobookPage";
import { audiobookProgress } from "../model/audiobookProgress";
import { audiobookPages } from "../fixtures/audiobooks";
import { openAddAudiobookDialog } from "../../../test/media";

vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({ mode: "browser", prepareSelection: vi.fn() }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));
afterEach(cleanup);

function inProgress() {
  const { detail } = audiobookPages();
  const chapters = detail.detail.chapters.slice(0, 3);
  const current = chapters[1];
  return {
    ...detail,
    detail: {
      ...detail.detail,
      chapters,
      bookmarks: [],
      continuation: {
        chapter: {
          audiobook_id: detail.detail.playlist.id,
          entry_id: current.entry_id,
          recording_id: current.recording_id!,
        },
        progress: {
          recording_id: current.recording_id!,
          position_ms: 60_000,
          speed: 1,
        },
      },
    },
  };
}

test("overall progress counts earlier chapters plus the saved position", () => {
  const props = inProgress();
  expect(audiobookProgress(props.detail)).toEqual({
    chapterNumber: 2,
    overall: { percent: 50, leftMs: 180_000 },
  });
  const unknown = {
    ...props.detail,
    chapters: props.detail.chapters.map((chapter, index) =>
      index === 2 ? { ...chapter, duration_ms: 0 } : chapter,
    ),
  };
  expect(audiobookProgress(unknown)).toEqual({
    chapterNumber: 2,
    overall: null,
  });
  expect(audiobookProgress({ ...props.detail, continuation: null })).toBeNull();
});

test("the audiobook header shows author, progress, Continue and the current chapter state", () => {
  render(createElement(AudiobookPage, inProgress()));
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
    "An owned audiobook",
  );
  expect(screen.getByText("Owned narrator")).toBeTruthy();
  expect(screen.getByText("50% · 3 min left")).toBeTruthy();
  expect(
    screen
      .getByRole("progressbar", { name: "Position in the audiobook" })
      .getAttribute("aria-valuenow"),
  ).toBe("50");
  expect(
    screen.getByRole("button", { name: "Continue ch. 2 · 1:00" }),
  ).toBeTruthy();
  expect(screen.getByRole("button", { name: "Bookmark" })).toBeTruthy();
  const rows = within(
    screen.getByRole("list", { name: "Chapters" }),
  ).getAllByRole("listitem");
  expect(within(rows[1]).getByText("Listening · 1:00")).toBeTruthy();
  expect(within(rows[0]).queryByText(/Listening/)).toBeNull();
  expect(
    within(rows[2]).getByRole("button", {
      name: "Play chapter 3 from beginning",
    }),
  ).toBeTruthy();
});

test("without saved progress there is no Continue button or progress bar", () => {
  const { detail } = audiobookPages();
  render(createElement(AudiobookPage, detail));
  expect(screen.queryByRole("button", { name: /^Continue/ })).toBeNull();
  expect(screen.queryByRole("progressbar")).toBeNull();
});

test("adding an audiobook opens a dialog that explains chapters come from playlist entries", () => {
  const { index } = audiobookPages();
  render(createElement(AudiobookIndexPage, index));
  expect(screen.queryByRole("dialog")).toBeNull();
  openAddAudiobookDialog();
  const dialog = screen.getByRole("dialog", { name: "Add an audiobook" });
  expect(
    within(dialog).getByText(/entries, in order, become the chapters/),
  ).toBeTruthy();
  expect(
    (
      within(dialog).getByRole("button", {
        name: "Add audiobook",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("Continue needs only saved progress, and an unknown duration hides the total", () => {
  const props = inProgress();
  const unknown = {
    ...props,
    detail: {
      ...props.detail,
      chapters: props.detail.chapters.map((chapter, index) =>
        index === 2 ? { ...chapter, duration_ms: 0 } : chapter,
      ),
    },
  };
  render(createElement(AudiobookPage, unknown));
  expect(
    screen.getByRole("button", { name: "Continue ch. 2 · 1:00" }),
  ).toBeTruthy();
  expect(screen.getByText(/^Audiobook · 3 chapters · ≥ /)).toBeTruthy();
  expect(screen.queryByRole("progressbar")).toBeNull();
});

test("the Bookmark button explains why it is disabled while the form is locked", () => {
  const { detail } = audiobookPages();
  render(
    createElement(AudiobookPage, {
      ...detail,
      detail: { ...detail.detail, chapters: [] },
    }),
  );
  const button = screen.getByRole("button", {
    name: "Bookmark",
  }) as HTMLButtonElement;
  expect(button.disabled).toBe(true);
  expect(button.title).toMatch(/chapter/i);
});

test("chapter rows keep their number and title together", () => {
  render(createElement(AudiobookPage, inProgress()));
  const rows = within(
    screen.getByRole("list", { name: "Chapters" }),
  ).getAllByRole("listitem");
  expect(
    within(rows[0]).getByText("1", { selector: ".chapter-number" }),
  ).toBeTruthy();
  expect(rows[0].getAttribute("aria-label")).toBe("Chapter 1: Owned chapter A");
});
