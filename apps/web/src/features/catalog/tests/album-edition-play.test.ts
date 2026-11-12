import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AlbumPage from "../pages/AlbumPage";
import {
  albumDetail,
  albumId,
  artistAssociation,
  artistReviewAccount,
  editionGroup,
  firstLibraryRecording,
  mergedTrackList,
  readyChoice,
  savedRecording,
  standardEdition,
} from "../fixtures/albumEditions";

type Selection = {
  queue: Schema<"Recording">[];
  index: number;
  more?: {
    next(signal: AbortSignal): Promise<Schema<"Recording">[]>;
  };
};

const player = vi.hoisted(() => {
  const select = vi.fn(async (_selection: unknown) => true);
  return {
    select,
    playQueue: vi.fn(),
    playSelection: vi.fn(),
    prepareSelection: vi.fn(() => select),
    shuffle: vi.fn(),
    subscribe: () => () => {},
    snapshot: () => ({ current: null, shuffle: false }),
    queueSnapshot: () => ({ currentIndex: null, total: 0 }),
  };
});
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn(), visit: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const savedRead = `/api/v1/albums/${albumId}/edition/track-list`;
const audioPath = `/api/v1/albums/${albumId}/edition-track-audio`;
const otherRelease = { ...firstLibraryRecording, release_id: "other-release" };
const editionList = {
  ...mergedTrackList,
  matches: [
    { occurrence_id: "occ-1-1", recording: firstLibraryRecording },
    { occurrence_id: "occ-2-1", recording: otherRelease },
  ],
} satisfies Schema<"AlbumEditionTrackListAvailability">;

let searchOn = true;
let audioBodies: Record<string, string>[] = [];
let savedReads = 0;

function serve() {
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    if (path === savedRead) {
      savedReads++;
      return Response.json(editionList);
    }
    if (path === audioPath) {
      const body = JSON.parse(init!.body as string);
      audioBodies.push(body);
      if (!searchOn) return Response.json({ status: "search_off" });
      return Response.json(
        body.occurrence_id === "occ-1-2"
          ? { status: "saved", recording: savedRecording }
          : { status: "no_match" },
      );
    }
    throw new Error(`Unexpected request ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

function page(saved: Schema<"AlbumEditionChoice"> = readyChoice) {
  return createElement(AlbumPage, {
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    user: { id: artistReviewAccount, username: "Listener", admin: false },
    libraries: [],
    playlists: [],
    detail: { ...albumDetail, saved_edition: saved },
  });
}

async function drain(selection: Selection) {
  const played = selection.queue.map((recording) => recording.id);
  for (;;) {
    const next = await selection.more!.next(new AbortController().signal);
    if (next.length === 0) return played;
    played.push(...next.map((recording) => recording.id));
  }
}

async function editionShown() {
  await waitFor(() => expect(savedReads).toBe(1));
  await screen.findByText("Catalog only song");
  const header = document.querySelector<HTMLElement>(".album-header")!;
  await waitFor(() =>
    expect(within(header).getByText(/4 tracks/)).toBeTruthy(),
  );
}

async function playFromHeader() {
  await editionShown();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Play album" }));
  });
  expect(player.select).toHaveBeenCalledTimes(1);
  return player.select.mock.calls[0][0] as Selection;
}

beforeEach(() => {
  searchOn = true;
  audioBodies = [];
  savedReads = 0;
  vi.clearAllMocks();
});
afterEach(() => vi.unstubAllGlobals());

test("header Play with a remembered edition plays owned and catalog tracks in edition order through the catalog search", async () => {
  serve();
  render(page());
  const selection = await playFromHeader();
  expect(selection.index).toBe(0);
  expect(selection.queue).toEqual([firstLibraryRecording]);
  expect(audioBodies).toEqual([]);
  expect(await drain(selection)).toEqual([
    firstLibraryRecording.id,
    savedRecording.id,
    otherRelease.id,
  ]);
  expect(audioBodies).toEqual([
    {
      recording_id: firstLibraryRecording.id,
      association_id: artistAssociation.id,
      release_group_id: editionGroup.id,
      edition_id: standardEdition.id,
      occurrence_id: "occ-1-2",
    },
    {
      recording_id: firstLibraryRecording.id,
      association_id: artistAssociation.id,
      release_group_id: editionGroup.id,
      edition_id: standardEdition.id,
      occurrence_id: "occ-2-2",
    },
  ]);
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("the album header counts the edition's tracks once the edition is shown", async () => {
  serve();
  render(page());
  const header = document.querySelector<HTMLElement>(".album-header")!;
  expect(header.textContent).toContain("2 tracks");
  await screen.findByText("Catalog only song");
  await waitFor(() =>
    expect(within(header).getByText(/4 tracks/)).toBeTruthy(),
  );
});

test("with automatic service search off, header Play keeps to the owned tracks in edition order and explains why", async () => {
  serve();
  searchOn = false;
  render(page());
  const selection = await playFromHeader();
  expect(selection.queue).toEqual([firstLibraryRecording]);
  expect(await drain(selection)).toEqual([
    firstLibraryRecording.id,
    otherRelease.id,
  ]);
  expect(audioBodies.map((body) => body.occurrence_id)).toEqual(["occ-1-2"]);
  expect(
    await screen.findByText(/Search connected services automatically/),
  ).toBeTruthy();
  expect(
    screen
      .getByRole("link", { name: "Turn it on in Settings" })
      .getAttribute("href"),
  ).toBe("/settings?section=privacy");
});

test("header Shuffle with an edition starts the edition through the catalog path and turns shuffle on", async () => {
  serve();
  render(page());
  await editionShown();
  const random = vi.spyOn(Math, "random").mockReturnValue(0);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Shuffle album" }));
  });
  await waitFor(() => expect(player.shuffle).toHaveBeenCalledTimes(1));
  expect(player.select).toHaveBeenCalledTimes(1);
  random.mockRestore();
  const selection = player.select.mock.calls[0][0] as Selection;
  expect(selection.queue.map((recording) => recording.id)).toEqual([
    firstLibraryRecording.id,
  ]);
  expect(audioBodies).toEqual([]);
  expect(await drain(selection)).toEqual([
    firstLibraryRecording.id,
    savedRecording.id,
    otherRelease.id,
  ]);
  expect(audioBodies.map((body) => body.occurrence_id)).toEqual([
    "occ-1-2",
    "occ-2-2",
  ]);
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("without an edition, header Play plays the library release tracks as before", async () => {
  const fetch = serve();
  render(page({ revision: 0, state: { status: "unset" } }));
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Play album" }));
  });
  expect(player.playQueue).toHaveBeenCalledTimes(1);
  expect(
    player.playQueue.mock.calls[0][0].map(
      (track: Schema<"Recording">) => track.id,
    ),
  ).toEqual(albumDetail.tracks.items.map((track) => track.id));
  expect(player.select).not.toHaveBeenCalled();
  expect(fetch).not.toHaveBeenCalled();
});
