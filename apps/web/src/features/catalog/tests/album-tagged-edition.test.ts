import type { Schema } from "../../../shared/schema";
import { editionChip, editionMenuItem } from "../fixtures/editionMenu";
import { createElement, type AnchorHTMLAttributes } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AlbumPage from "../pages/AlbumPage";

import {
  albumDetail,
  albumId,
  artistAssociation,
  artistReviewAccount,
  editionGroup,
  firstDiscographyPage,
  firstEditionPage,
  firstLibraryRecording,
  mergedTrackList,
  readyChoice,
  reviewedArtistProfile,
  savedEdition,
} from "../fixtures/albumEditions";

vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue: vi.fn(),
    playSelection: vi.fn(),
    prepareSelection: vi.fn(),
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  }),
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const associations = `/api/v1/recordings/${firstLibraryRecording.id}/artist-associations`;
const tagged = `/api/v1/albums/${albumId}/tagged-edition`;
const comparison = `/api/v1/albums/${albumId}/edition-track-list`;

type Route = (init?: RequestInit) => Response | Promise<Response>;

function serve(answer: Route) {
  const routes: Record<string, Route> = {
    [tagged]: answer,
    [associations]: () => Response.json([artistAssociation]),
    [`${associations}/${artistAssociation.id}/profile`]: () =>
      Response.json(reviewedArtistProfile),
    [`${associations}/${artistAssociation.id}/discography`]: () =>
      Response.json(firstDiscographyPage),
    [`${associations}/${artistAssociation.id}/release-group-editions`]: () =>
      Response.json({ ...firstEditionPage, continuation: null }),
    [comparison]: () => Response.json(mergedTrackList),
  };
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    const route = routes[path];
    if (!route) throw new Error(`Unexpected request ${path}`);
    return route(init);
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

function page(saved: Schema<"AlbumEditionChoice">) {
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

const unset: Schema<"AlbumEditionChoice"> = {
  revision: 0,
  state: { status: "unset" },
};
const paths = (fetch: ReturnType<typeof serve>) =>
  fetch.mock.calls.map(([path]) => path);

afterEach(() => vi.unstubAllGlobals());

test("an album with a tagged release id opens on that exact edition without browsing", async () => {
  const fetch = serve(() =>
    Response.json({ status: "found", edition: savedEdition }),
  );
  render(page(unset));
  expect(fetch).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  const region = await screen.findByRole("region", {
    name: "Album edition track list",
  });
  expect(await within(region).findByText("Catalog only song")).toBeTruthy();
  expect(paths(fetch)).toEqual([tagged, comparison]);
  expect(fetch.mock.calls[0][1]!.method).toBe("POST");
  expect(JSON.parse(fetch.mock.calls[1][1]!.body as string)).toEqual({
    recording_id: savedEdition.recording_id,
    association_id: savedEdition.association_id,
    release_group_id: editionGroup.id,
    edition_id: savedEdition.edition_id,
  });
  expect(
    screen.getByText(/edition named by the release id in your file tags/),
  ).toBeTruthy();
  expect(editionChip()).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Forget edition" })).toBeNull();
});

test("without a usable tagged edition the discography browser opens as before", async () => {
  for (const answer of [
    () => Response.json({ status: "none" }),
    () => Response.json({ error: "busy" }, { status: 500 }),
  ]) {
    const fetch = serve(answer);
    const view = render(page(unset));
    fireEvent.click(
      screen.getByRole("button", { name: "Show complete track list" }),
    );
    await screen.findByRole("heading", {
      name: "Which release is Catalog album?",
    });
    expect(paths(fetch)[0]).toBe(tagged);
    expect(paths(fetch)).not.toContain(comparison);
    view.unmount();
    vi.unstubAllGlobals();
  }
});

test("an owner's choice or an earlier decision never asks for the tagged edition", async () => {
  const decided: Schema<"AlbumEditionChoice"> = {
    revision: 2,
    state: { status: "unset" },
  };
  for (const saved of [readyChoice, decided]) {
    const fetch = serve(() => Response.json({ status: "none" }));
    const view = render(page(saved));
    fireEvent.click(
      screen.getByRole("button", { name: "Show complete track list" }),
    );
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(paths(fetch)).not.toContain(tagged);
    view.unmount();
    vi.unstubAllGlobals();
  }
});

test("leaving the page while the lookup is pending aborts its request", async () => {
  let signal: AbortSignal | null | undefined;
  const fetch = serve((init) => {
    signal = init?.signal;
    return new Promise<Response>(() => {});
  });
  const view = render(page(unset));
  fireEvent.click(
    screen.getByRole("button", { name: "Show complete track list" }),
  );
  await waitFor(() => expect(paths(fetch)).toContain(tagged));
  expect(signal?.aborted).toBe(false);
  view.unmount();
  expect(signal?.aborted).toBe(true);
});
