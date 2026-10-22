import { createElement, type AnchorHTMLAttributes } from "react";
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
vi.mock("../../playback/components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate, pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: navigation,
  usePage: () => ({ url: window.location.pathname + window.location.search }),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));
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
  expect(within(rows[1]).getByText("Unavailable on this device")).toBeTruthy();
  for (const [index, member] of detail.detail.recordings.items.entries()) {
    expect(
      within(rows[index]).getByRole("button", { name: member.recording.title }),
    ).toBeTruthy();
    expect(
      within(rows[index])
        .getByRole("link", { name: `Details for ${member.recording.title}` })
        .getAttribute("href"),
    ).toBe(`/recordings/${member.recording.id}`);
  }
  expect(
    screen
      .getByRole("link", { name: "Saved album artist" })
      .getAttribute("href"),
  ).toBe("/artists/imported-album-credit");
  expect(
    screen.getByRole("link", { name: "Open Local album" }).getAttribute("href"),
  ).toBe("/albums/owned-local-album");
  expect(screen.getByText("Album artist not provided")).toBeTruthy();
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
  expect(fetch).not.toHaveBeenCalled();
  expect(audio).not.toHaveBeenCalled();
  expect(mutate).not.toHaveBeenCalled();
  expect(player.playQueue).not.toHaveBeenCalled();
  expect(player.subscribePosition).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Play this page" }));
  expect(player.playQueue).toHaveBeenLastCalledWith(
    detail.detail.recordings.items.map(({ recording }) => ({
      ...recording,
      row_id: recording.id,
    })),
  );
  fireEvent.click(
    within(rows[1]).getByRole("button", {
      name: `Play ${detail.detail.recordings.items[1].recording.title}`,
    }),
  );
  expect(player.playQueue).toHaveBeenLastCalledWith(
    expect.any(Array),
    1,
    undefined,
  );
  expect(detail).toEqual(before);
});

test("chosen artists stay beside every library credit and page independently", () => {
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
  const credits = screen.getByRole("region", { name: "Library credits" });
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
  fireEvent.click(within(chosen).getByRole("button", { name: "Next" }));
  expect(navigation.get).toHaveBeenLastCalledWith("/artists", {
    q: "Saved",
    library: "owned-local-library",
    limit: "2",
    offset: "0",
    group_offset: 2,
  });
  fireEvent.click(within(credits).getByRole("button", { name: "Next" }));
  expect(navigation.get).toHaveBeenLastCalledWith("/artists", {
    q: "Saved",
    library: "owned-local-library",
    limit: "2",
    offset: 2,
    group_offset: "0",
  });
  expect(fetch).not.toHaveBeenCalled();
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
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(Sidebar, {
        user: index.user,
        playlists: index.playlists,
        path: `/artist-groups/${group.id}`,
      }),
    ),
  );
  const navigation = screen.getByRole("navigation", {
    name: "Main navigation",
  });
  expect(
    within(navigation)
      .getByRole("link", { name: "Artists" })
      .getAttribute("aria-current"),
  ).toBe("page");
  expect(
    within(navigation)
      .getByRole("link", { name: "Library" })
      .getAttribute("aria-current"),
  ).toBeNull();
});

test("an emptied chosen-artist page keeps its previous page and every legacy credit", () => {
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
    within(chosen).getByRole("heading", { name: "No matching chosen artists" }),
  ).toBeTruthy();
  expect(
    within(
      screen.getByRole("region", { name: "Library credits" }),
    ).getAllByRole("link"),
  ).toHaveLength(2);
  fireEvent.click(within(chosen).getByRole("button", { name: "Previous" }));
  expect(navigation.get).toHaveBeenLastCalledWith("/artists", {
    q: "Saved",
    library: "owned-local-library",
    limit: "2",
    offset: "2",
    group_offset: 2,
  });
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
