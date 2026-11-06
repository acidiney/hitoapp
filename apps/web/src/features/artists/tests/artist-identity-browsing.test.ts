import { createElement, useState, type AnchorHTMLAttributes } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistIndexPage from "../pages/ArtistIndexPage";
import ArtistIdentityGroupPage from "../pages/ArtistIdentityGroupPage";
import Sidebar from "../../../app/Sidebar";
import LibrarySections from "../../../app/LibrarySections";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { artistIdentityBrowsing } from "../fixtures/artistIdentityBrowsing";

const navigation = vi.hoisted(() => ({ get: vi.fn() }));
const player = vi.hoisted(() => ({
  playQueue: vi.fn(),
  subscribe: () => () => {},
  snapshot: () => ({ current: null }),
  subscribePosition: vi.fn(),
}));
const mutate = vi.hoisted(() => vi.fn());
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate, pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: navigation,
  usePage: () => ({ url: window.location.pathname + window.location.search }),
  useRemember: (initial: unknown) => useState(initial),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));
function expectOnlyMatching(fetch: { mock: { calls: unknown[][] } }) {
  for (const [path, options] of fetch.mock.calls) {
    expect(
      [
        "/api/v1/artist-matching/status",
        "/api/v1/artist-matching/review-queue?limit=50",
        "/api/v1/artist-matching/review-queue/dismissed?limit=50",
      ],
      "only the status and queue reads, never a pass or provider request",
    ).toContain(path);
    expect((options as RequestInit | undefined)?.method ?? "GET").toBe("GET");
  }
}
afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
});

test("a chosen artist uses its saved credit and original recordings with independent member pages", () => {
  const { detail, group } = artistIdentityBrowsing();
  const before = structuredClone(detail);
  const fetch = vi.fn();
  const audio = vi.fn();
  vi.stubGlobal("fetch", fetch);
  vi.stubGlobal("Audio", audio);
  const path = `/artist-groups/${group.id}`;
  window.history.replaceState(
    null,
    "",
    `${path}?library=owned-local-library&limit=2&library_offset=0&recording_offset=0&release_offset=0`,
  );
  render(createElement(ArtistIdentityGroupPage, detail));
  expect(
    screen.getByRole("heading", { name: group.name.value, level: 1 }),
  ).toBeTruthy();
  expect(screen.getByText("Chosen artist", { exact: true })).toBeTruthy();
  const table = screen.getByRole("table", { name: "Tracks" });
  const rows = within(table).getAllByRole("row").slice(1);
  expect(rows).toHaveLength(2);
  expect(within(rows[1]).getByText("Plays from your services")).toBeTruthy();
  for (const [index, member] of detail.detail.recordings.items.entries()) {
    expect(
      within(rows[index])
        .getByRole("link", { name: member.recording.title })
        .getAttribute("href"),
    ).toBe(`/recordings/${member.recording.id}`);
    expect(
      within(rows[index]).getByRole("button", {
        name: `Actions for ${member.recording.title}`,
      }),
    ).toBeTruthy();
  }
  expect(
    screen
      .getByRole("link", { name: "Saved album artist" })
      .getAttribute("href"),
  ).toBe("/artists/imported-album-credit");
  expect(
    screen.getByRole("link", { name: "Open Local album" }).getAttribute("href"),
  ).toBe("/albums/owned-local-album");
  expect(screen.queryByText(/not provided/)).toBeNull();
  for (const [label, parameter] of [
    ["Library pages", "library_offset"],
    ["Track pages", "recording_offset"],
    ["Album pages", "release_offset"],
  ]) {
    fireEvent.click(
      within(screen.getByRole("navigation", { name: label })).getByRole(
        "button",
        { name: "Next" },
      ),
    );
    expect(navigation.get).toHaveBeenLastCalledWith(path, {
      library: "owned-local-library",
      limit: "2",
      library_offset: "0",
      recording_offset: "0",
      release_offset: "0",
      [parameter]: 2,
    });
  }
  expectOnlyMatching(fetch);
  expect(audio).not.toHaveBeenCalled();
  expect(mutate).not.toHaveBeenCalled();
  expect(player.playQueue).not.toHaveBeenCalled();
  expect(player.subscribePosition).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  expect(player.playQueue).toHaveBeenLastCalledWith(
    detail.detail.recordings.items.map(({ recording }) => ({
      ...recording,
      row_id: recording.id,
    })),
    0,
    undefined,
    expect.objectContaining({ next: expect.any(Function) }),
  );
  expect(player.playQueue.mock.lastCall![3].remaining()).toBe(1);
  fireEvent.click(
    within(rows[1]).getByRole("button", {
      name: `Play ${detail.detail.recordings.items[1].recording.title}`,
    }),
  );
  expect(player.playQueue).toHaveBeenLastCalledWith(
    expect.any(Array),
    1,
    undefined,
    expect.objectContaining({ next: expect.any(Function) }),
  );
  expect(detail).toEqual(before);
});

test("chosen artists stay beside the library credits they leave listed and page independently", () => {
  const { index, group } = artistIdentityBrowsing();
  const before = structuredClone(index);
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  window.history.replaceState(
    null,
    "",
    "/artists?q=Saved&library=owned-local-library&limit=2&offset=0&group_offset=0",
  );
  render(createElement(ArtistIndexPage, index));
  expect(screen.getByRole("heading", { name: "Chosen artists" })).toBeTruthy();
  const chosen = screen.getByRole("region", { name: "Chosen artists" });
  const credits = screen.getByRole("region", {
    name: "Artists in your library",
  });
  expect(
    within(credits)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href")),
  ).toEqual(index.artists.items.map((artist) => `/artists/${artist.id}`));
  expect(
    within(chosen)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href")),
  ).toEqual([
    `/artist-groups/${group.id}?library=owned-local-library&limit=2`,
    "/artist-groups/other-chosen-group?library=owned-local-library&limit=2",
  ]);
  for (const region of [chosen, credits]) {
    expect(within(region).queryByRole("button", { name: "Next" })).toBeNull();
    expect(
      within(region).getByRole("button", { name: "Load more" }),
    ).toBeTruthy();
  }
  expect(navigation.get).not.toHaveBeenCalled();
  expectOnlyMatching(fetch);
  expect(index).toEqual(before);
});

test("group library filters use authorized names and reset only the detail pages", () => {
  const { detail, group } = artistIdentityBrowsing();
  detail.libraries = detail.libraries.filter(
    (library) => library.id === "owned-local-library",
  );
  const before = structuredClone(detail);
  const path = `/artist-groups/${group.id}`;
  window.history.replaceState(
    null,
    "",
    `${path}?library=owned-local-library&limit=2&library_offset=4&recording_offset=6&release_offset=8&q=IndexOnly`,
  );
  render(createElement(ArtistIdentityGroupPage, detail));
  const libraries = screen.getByRole("region", { name: "Libraries" });
  expect(
    within(libraries)
      .getByRole("link", { name: "Local library" })
      .getAttribute("href"),
  ).toBe(`${path}?library=owned-local-library&limit=2`);
  expect(
    within(libraries)
      .getByRole("link", { name: "Library" })
      .getAttribute("href"),
  ).toBe(
    `${path}?library=${detail.detail.libraries.items[1].library_id}&limit=2`,
  );
  expect(
    within(libraries)
      .getByRole("link", { name: "All libraries" })
      .getAttribute("href"),
  ).toBe(`${path}?limit=2`);
  expect(libraries.textContent).not.toContain(
    detail.detail.libraries.items[1].library_id,
  );
  expect(libraries.textContent).not.toContain("Imported library");
  expect(libraries.textContent).not.toContain(group.provider_artist_id);
  fireEvent.click(within(libraries).getByRole("button", { name: "Next" }));
  expect(navigation.get).toHaveBeenLastCalledWith(path, {
    library: "owned-local-library",
    limit: "2",
    library_offset: 2,
    recording_offset: "6",
    release_offset: "8",
    q: "IndexOnly",
  });
  expect(detail).toEqual(before);
});

test("group detail stays within Artists navigation", () => {
  const { index, group } = artistIdentityBrowsing();
  const path = `/artist-groups/${group.id}`;
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(Sidebar, {
        playlists: index.playlists,
        path,
      }),
      createElement(LibrarySections, { path }),
    ),
  );
  const sections = screen.getByRole("navigation", {
    name: "Library sections",
  });
  expect(
    within(sections)
      .getByRole("link", { name: "Artists" })
      .getAttribute("aria-current"),
  ).toBe("page");
  expect(
    within(sections)
      .getByRole("link", { name: "Library" })
      .getAttribute("aria-current"),
  ).toBeNull();
  expect(
    within(screen.getByRole("navigation", { name: "Main navigation" }))
      .getByRole("link", { name: "Library" })
      .getAttribute("aria-current"),
  ).toBe("page");
});

test("an emptied chosen-artist page keeps its previous page and the listed library credits", () => {
  const { index } = artistIdentityBrowsing();
  index.identity_groups.items = [];
  index.identity_groups.offset = 4;
  index.identity_groups.total = 1;
  window.history.replaceState(
    null,
    "",
    "/artists?q=Saved&library=owned-local-library&limit=2&offset=2&group_offset=4",
  );
  render(createElement(ArtistIndexPage, index));
  const chosen = screen.getByRole("region", { name: "Chosen artists" });
  expect(
    within(chosen).queryByRole("heading", { level: 2, name: /No / }),
  ).toBeNull();
  expect(
    within(
      screen.getByRole("region", { name: "Artists in your library" }),
    ).getAllByRole("link"),
  ).toHaveLength(2);
  expect(
    within(chosen)
      .getByRole("link", { name: "Show the first chosen artists" })
      .getAttribute("href"),
  ).toBe(
    `/artists?${new URLSearchParams({ q: index.query, library: "owned-local-library", limit: "2" })}`,
  );
});

test("group entry links drop unauthorized filters and all index paging and search", () => {
  const { index, group } = artistIdentityBrowsing();
  window.history.replaceState(
    null,
    "",
    "/artists?library=not-currently-authorized&q=Private&offset=20&group_offset=10&recording_offset=4&release_offset=6",
  );
  render(createElement(ArtistIndexPage, index));
  const chosen = screen.getByRole("region", { name: "Chosen artists" });
  expect(
    within(chosen)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href")),
  ).toEqual([
    `/artist-groups/${group.id}`,
    "/artist-groups/other-chosen-group",
  ]);
});

test("empty group member pages retain independent previous-page controls without starting playback", () => {
  const { detail, group } = artistIdentityBrowsing();
  for (const page of [
    detail.detail.libraries,
    detail.detail.recordings,
    detail.detail.releases,
  ]) {
    page.items = [];
    page.offset = 4;
    page.total = 1;
  }
  const path = `/artist-groups/${group.id}`;
  window.history.replaceState(
    null,
    "",
    `${path}?limit=2&library_offset=4&recording_offset=4&release_offset=4`,
  );
  render(createElement(ArtistIdentityGroupPage, detail));
  for (const [label, parameter] of [
    ["Library pages", "library_offset"],
    ["Track pages", "recording_offset"],
    ["Album pages", "release_offset"],
  ]) {
    const pages = screen.getByRole("navigation", { name: label });
    expect(
      within(pages)
        .getByRole("button", { name: "Next" })
        .hasAttribute("disabled"),
    ).toBe(true);
    fireEvent.click(within(pages).getByRole("button", { name: "Previous" }));
    expect(navigation.get).toHaveBeenLastCalledWith(path, {
      limit: "2",
      library_offset: "4",
      recording_offset: "4",
      release_offset: "4",
      [parameter]: 2,
    });
  }
  expect(screen.queryByRole("table")).toBeNull();
  expect(screen.queryByRole("button", { name: /Play/ })).toBeNull();
  expect(player.playQueue).not.toHaveBeenCalled();
  expect(mutate).not.toHaveBeenCalled();
});
