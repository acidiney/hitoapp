import { createElement, type AnchorHTMLAttributes } from "react";
import {
  act,
  fireEvent,
  cleanup,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistDiscography from "../components/ArtistDiscography";
import { firstDiscographyPage } from "../fixtures/artistDiscography";
import { editionGroup } from "../fixtures/artistEditions";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewDetail,
  reviewedArtistProfile,
} from "../fixtures/artistReview";
import {
  catalogAlbumEdition,
  catalogAlbumTitles,
  catalogAlbumTrackList,
  foundRecording,
  oneEditionPage,
  secondEdition,
  twoEditionsPage,
} from "../fixtures/catalogAlbumPlay";

const player = vi.hoisted(() => {
  const queue = { total: 1, currentIndex: 0 as number | null };
  const listeners = new Set<() => void>();
  const selected = vi.fn(async (_target: unknown) => true);
  return {
    queue,
    listeners,
    selected,
    prepareSelection: vi.fn(() => selected),
    playQueue: vi.fn(),
    shuffle: vi.fn(),
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    snapshot: () => ({ current: null, shuffle: false }),
    queueSnapshot: () => ({
      version: 0,
      total: queue.total,
      currentIndex: queue.currentIndex,
      currentEntry: null,
      pending: 0,
    }),
  };
});
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const base = `/api/v1/recordings/${artistReviewDetail.recording_id}/artist-associations/${artistAssociation.id}`;
const audioPath = `${base}/edition-track-audio`;
const notFound = new Set<string>();
const audioBodies: Array<{ occurrence_id: string }> = [];
const control: {
  searchOn: boolean;
  failing: boolean;
  hold: Promise<void> | null;
} = { searchOn: true, failing: false, hold: null };

afterEach(() => {
  notFound.clear();
  audioBodies.length = 0;
  control.searchOn = true;
  control.failing = false;
  control.hold = null;
  player.queue.total = 1;
  player.queue.currentIndex = 0;
  player.selected.mockClear();
  player.prepareSelection.mockClear();
  vi.unstubAllGlobals();
});

function serve(options: { editions?: unknown } = {}) {
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    if (path === `${base}/discography`)
      return Response.json(firstDiscographyPage);
    if (path === `${base}/release-group-editions`)
      return Response.json(options.editions ?? oneEditionPage);
    if (path === `${base}/edition-track-list`) {
      const body = JSON.parse(init!.body as string);
      return Response.json({
        ...catalogAlbumTrackList,
        edition: { ...catalogAlbumEdition, id: body.edition_id },
      });
    }
    if (path === audioPath) {
      const body = JSON.parse(init!.body as string);
      audioBodies.push(body);
      if (control.hold) await control.hold;
      if (!control.searchOn) return Response.json({ status: "search_off" });
      if (control.failing) return Response.json({ status: "unavailable" });
      const index = Number(body.occurrence_id.split("/").pop()) - 1;
      return Response.json(
        notFound.has(body.occurrence_id)
          ? { status: "no_match" }
          : { status: "saved", recording: foundRecording(index) },
      );
    }
    throw new Error(`Unexpected request ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

async function openCard() {
  render(
    createElement(ArtistDiscography, {
      account: artistReviewAccount,
      recording: artistReviewDetail.recording_id,
      association: artistAssociation,
      profile: reviewedArtistProfile.profile,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Browse discography" }));
  const groups = await screen.findByRole("list", {
    name: "Discography releases",
  });
  return within(await within(groups).findByRole("listitem"));
}

async function openAlbum() {
  const card = await openCard();
  fireEvent.click(card.getByRole("button", { name: "Open album" }));
  await card.findByRole("button", { name: `Play ${catalogAlbumTitles[0]}` });
  return card;
}

const occurrence = (index: number) => `opaque:occurrence/play/${index + 1}`;

test("an opened catalog album names itself once, hides a lone edition and the unchecked-audio note, and lists playable rows", async () => {
  serve();
  const card = await openAlbum();
  expect(
    card.getAllByRole("heading", { name: editionGroup.title }),
  ).toHaveLength(1);
  expect(card.queryByRole("region", { name: "Editions" })).toBeNull();
  expect(card.queryByRole("button", { name: "Choose edition" })).toBeNull();
  expect(
    card.queryByText(/Audio availability has not been checked/),
  ).toBeNull();
  expect(card.queryByText("Catalog only")).toBeNull();
  const table = card.getByRole("table", { name: "Tracks" });
  const rows = within(table).getAllByRole("row").slice(1);
  expect(rows).toHaveLength(4);
  expect(within(rows[0]).getByText("1:01")).toBeTruthy();
  expect(card.getByRole("button", { name: "Play album" })).toBeTruthy();
  expect(card.getByRole("button", { name: "Shuffle album" })).toBeTruthy();
});

test("two editions offer one compact switcher that reads the other edition's tracks", async () => {
  const fetch = serve({ editions: twoEditionsPage });
  const card = await openAlbum();
  const switcher = card.getByRole("combobox", { name: "Edition" });
  expect(within(switcher).getAllByRole("option")).toHaveLength(2);
  fireEvent.change(switcher, { target: { value: secondEdition.id } });
  await waitFor(() =>
    expect(
      fetch.mock.calls
        .filter(([path]) => path === `${base}/edition-track-list`)
        .map(([, init]) => JSON.parse(init!.body as string).edition_id),
    ).toEqual([twoEditionsPage.page.items[0].id, secondEdition.id]),
  );
});

test("Play resolves only the first track now, queues it and lets the rest follow one by one", async () => {
  const fetch = serve();
  notFound.add(occurrence(2));
  const card = await openAlbum();
  await act(async () => {
    fireEvent.click(card.getByRole("button", { name: "Play album" }));
  });
  await waitFor(() => expect(player.selected).toHaveBeenCalledTimes(1));
  expect(audioBodies.map((body) => body.occurrence_id)).toEqual([
    occurrence(0),
  ]);
  const selection = player.selected.mock.calls[0][0] as {
    queue: unknown[];
    index: number;
    more: {
      remaining(): number;
      next(signal: AbortSignal): Promise<unknown[]>;
    };
  };
  expect(selection.queue).toEqual([foundRecording(0)]);
  expect(selection.index).toBe(0);
  expect(selection.more.remaining()).toBe(3);

  const signal = new AbortController().signal;
  expect(await selection.more.next(signal)).toEqual([foundRecording(1)]);
  player.queue.total = 2;
  expect(await selection.more.next(signal)).toEqual([foundRecording(3)]);
  expect(audioBodies.map((body) => body.occurrence_id)).toEqual([
    occurrence(0),
    occurrence(1),
    occurrence(2),
    occurrence(3),
  ]);
  await waitFor(() => {
    const skipped = card
      .getAllByRole("row")
      .find((row) => row.textContent?.includes(catalogAlbumTitles[2]))!;
    expect(
      within(skipped).getByText("Not found on your services"),
    ).toBeTruthy();
  });
  expect(selection.more.remaining()).toBe(0);
  expect(fetch).toHaveBeenCalled();
});

test("the supply waits while enough upcoming tracks are queued and resumes when playback advances", async () => {
  serve();
  const card = await openAlbum();
  await act(async () => {
    fireEvent.click(card.getByRole("button", { name: "Play album" }));
  });
  await waitFor(() => expect(player.selected).toHaveBeenCalledTimes(1));
  const selection = player.selected.mock.calls[0][0] as {
    more: { next(signal: AbortSignal): Promise<unknown[]> };
  };
  player.queue.total = 4;
  player.queue.currentIndex = 0;
  const waiting = selection.more.next(new AbortController().signal);
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
  expect(audioBodies).toHaveLength(1);
  player.queue.currentIndex = 2;
  await act(async () => player.listeners.forEach((listener) => listener()));
  expect(await waiting).toEqual([foundRecording(1)]);
  expect(audioBodies).toHaveLength(2);
});

test("clicking a track row plays from that track and a first track that is not found falls through to the next", async () => {
  serve();
  notFound.add(occurrence(1));
  const card = await openAlbum();
  await act(async () => {
    fireEvent.click(
      card.getByRole("button", { name: `Play ${catalogAlbumTitles[1]}` }),
    );
  });
  await waitFor(() => expect(player.selected).toHaveBeenCalledTimes(1));
  expect(audioBodies.map((body) => body.occurrence_id)).toEqual([
    occurrence(1),
    occurrence(2),
  ]);
  const selection = player.selected.mock.calls[0][0] as { queue: unknown[] };
  expect(selection.queue).toEqual([foundRecording(2)]);
  expect(card.getByText("Not found on your services")).toBeTruthy();
});

test("without the automatic service search choice Play explains itself, links to the setting and Shuffle leaves the player's shuffle alone", async () => {
  serve();
  control.searchOn = false;
  const card = await openAlbum();
  await act(async () => {
    fireEvent.click(card.getByRole("button", { name: "Play album" }));
  });
  expect(
    await card.findByText(/Search connected services automatically/),
  ).toBeTruthy();
  expect(
    card
      .getByRole("link", { name: "Turn it on in Settings" })
      .getAttribute("href"),
  ).toBe("/settings?section=privacy");
  expect(player.selected).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.click(card.getByRole("button", { name: "Shuffle album" }));
  });
  await waitFor(() => expect(audioBodies.length).toBeGreaterThan(1));
  expect(player.shuffle).not.toHaveBeenCalled();
  expect(player.selected).not.toHaveBeenCalled();
});

test("Shuffle turns the player's shuffle on once playback began", async () => {
  serve();
  const card = await openAlbum();
  await act(async () => {
    fireEvent.click(card.getByRole("button", { name: "Shuffle album" }));
  });
  await waitFor(() => expect(player.shuffle).toHaveBeenCalledTimes(1));
  expect(player.selected).toHaveBeenCalledTimes(1);
});

test("turning the choice off while an album plays stops its queue supply with the same explanation", async () => {
  serve();
  const card = await openAlbum();
  await act(async () => {
    fireEvent.click(card.getByRole("button", { name: "Play album" }));
  });
  await waitFor(() => expect(player.selected).toHaveBeenCalledTimes(1));
  const selection = player.selected.mock.calls[0][0] as {
    more: { next(signal: AbortSignal): Promise<unknown[]> };
  };
  control.searchOn = false;
  expect(await selection.more.next(new AbortController().signal)).toEqual([]);
  expect(
    await card.findByText(/Search connected services automatically/),
  ).toBeTruthy();
  expect(audioBodies).toHaveLength(2);
});

test("three failed searches in a row stop the album with one quiet message and no further search", async () => {
  serve();
  control.failing = true;
  const card = await openAlbum();
  await act(async () => {
    fireEvent.click(card.getByRole("button", { name: "Play album" }));
  });
  expect(await card.findByText(/not answering right now/)).toBeTruthy();
  expect(audioBodies).toHaveLength(3);
  expect(player.selected).not.toHaveBeenCalled();
});

test("leaving the page while the first track is being searched starts no playback", async () => {
  serve();
  let release!: () => void;
  control.hold = new Promise<void>((resolve) => {
    release = resolve;
  });
  const card = await openAlbum();
  await act(async () => {
    fireEvent.click(card.getByRole("button", { name: "Play album" }));
  });
  await waitFor(() => expect(audioBodies).toHaveLength(1));
  cleanup();
  release();
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
  expect(player.selected).not.toHaveBeenCalled();
  expect(audioBodies).toHaveLength(1);
});

test("once playback started, leaving the page or switching edition does not stop the album's queue supply", async () => {
  serve({ editions: twoEditionsPage });
  const card = await openAlbum();
  await act(async () => {
    fireEvent.click(card.getByRole("button", { name: "Play album" }));
  });
  await waitFor(() => expect(player.selected).toHaveBeenCalledTimes(1));
  const selection = player.selected.mock.calls[0][0] as {
    more: { next(signal: AbortSignal): Promise<unknown[]> };
  };
  fireEvent.change(card.getByRole("combobox", { name: "Edition" }), {
    target: { value: secondEdition.id },
  });
  expect(await selection.more.next(new AbortController().signal)).toEqual([
    foundRecording(1),
  ]);
  cleanup();
  player.queue.total = 2;
  expect(await selection.more.next(new AbortController().signal)).toEqual([
    foundRecording(2),
  ]);
  expect(audioBodies).toHaveLength(3);
});

test("switching edition while a track is being searched retires the old search", async () => {
  serve({ editions: twoEditionsPage });
  let release!: () => void;
  control.hold = new Promise<void>((resolve) => {
    release = resolve;
  });
  const card = await openAlbum();
  await act(async () => {
    fireEvent.click(card.getByRole("button", { name: "Play album" }));
  });
  await waitFor(() => expect(audioBodies).toHaveLength(1));
  fireEvent.change(card.getByRole("combobox", { name: "Edition" }), {
    target: { value: secondEdition.id },
  });
  release();
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
  expect(player.selected).not.toHaveBeenCalled();
  expect(audioBodies).toHaveLength(1);
});

test("a service that is not connected stops the album with one honest message", async () => {
  serve();
  const fetch = vi.mocked(globalThis.fetch);
  const original = fetch.getMockImplementation()!;
  fetch.mockImplementation(async (path, init) =>
    path === audioPath
      ? Response.json({ status: "no_providers" })
      : original(path, init),
  );
  const card = await openAlbum();
  await act(async () => {
    fireEvent.click(card.getByRole("button", { name: "Play album" }));
  });
  expect(
    await card.findByText(/Connect an audio provider in Settings/),
  ).toBeTruthy();
  expect(player.selected).not.toHaveBeenCalled();
});
