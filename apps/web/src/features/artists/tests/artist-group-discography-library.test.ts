import { createElement, type AnchorHTMLAttributes } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistIdentityGroupPage from "../pages/ArtistIdentityGroupPage";
import { artistIdentityBrowsing } from "../fixtures/artistIdentityBrowsing";
import { artistGroupHttp } from "../fixtures/artistGroupDiscography";
import {
  artistAssociation,
  reviewedArtistProfile,
} from "../fixtures/artistReview";
import type { CatalogReleaseGroup } from "../../../shared/types";

const player = vi.hoisted(() => ({
  playQueue: vi.fn(),
  subscribe: () => () => {},
  snapshot: () => ({ current: null }),
  subscribePosition: vi.fn(),
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  usePage: () => ({ url: window.location.pathname + window.location.search }),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
});

function release(
  id: string,
  kind: CatalogReleaseGroup["kind"],
  title = `Title of ${id}`,
): CatalogReleaseGroup {
  return {
    id,
    title,
    artist_credit: null,
    kind,
    secondary_types: [],
    first_release_date: null,
    revision: null,
  };
}

function page(items: CatalogReleaseGroup[], continuation: string | null) {
  return {
    status: "available",
    association_id: artistAssociation.id,
    continuation,
    page: {
      artist_id: reviewedArtistProfile.profile.artist.id,
      snapshot: null,
      total_count: null,
      next_cursor: null,
      items,
    },
  };
}

async function open() {
  const { detail } = artistIdentityBrowsing();
  render(createElement(ArtistIdentityGroupPage, detail));
  fireEvent.click(
    screen.getByRole("button", { name: "Browse full discography" }),
  );
  return screen.findByRole("region", { name: "Discography" });
}

test("releases are grouped by primary type with exact library availability and no automatic import", async () => {
  const http = artistGroupHttp();
  http.reply(
    "discography",
    page(
      [
        release("rg:album-linked", "album", "Same title"),
        release("rg:album-other", "album", "Same title"),
        release("rg:ep", "ep"),
        release("rg:single", "single"),
        release("rg:live", "broadcast"),
        release("rg:unknown", null),
      ],
      null,
    ),
  );
  http.replyLibrary({
    association_id: artistAssociation.id,
    groups: [
      {
        release_group_id: "rg:album-linked",
        albums: [
          {
            release_id: "local album/é",
            edition_id: "edition:1",
            track_count: 10,
            playable_count: 7,
          },
        ],
      },
      { release_group_id: "rg:album-other", albums: [] },
      { release_group_id: "rg:ep", albums: [] },
      { release_group_id: "rg:single", albums: [] },
      { release_group_id: "rg:live", albums: [] },
      { release_group_id: "rg:unknown", albums: [] },
    ],
  });
  const region = await open();
  await within(region).findByText("7 of 10 tracks playable");
  expect(http.libraryRequests).toEqual([
    [
      "rg:album-linked",
      "rg:album-other",
      "rg:ep",
      "rg:single",
      "rg:live",
      "rg:unknown",
    ],
  ]);
  const headings = within(region)
    .getAllByRole("heading", { level: 4 })
    .map((heading) => heading.textContent);
  expect(headings.slice(0, 4)).toEqual([
    "Albums (2)",
    "EPs (1)",
    "Singles (1)",
    "Other releases (2)",
  ]);
  const albums = Array.from(
    within(region)
      .getByRole("region", { name: "Albums" })
      .querySelector(":scope > ul")!.children,
  ) as HTMLElement[];
  expect(albums).toHaveLength(2);
  const link = within(albums[0]).getByRole("link", {
    name: "Open in your library",
  });
  expect(link.getAttribute("href")).toBe("/albums/local%20album%2F%C3%A9");
  expect(
    within(albums[1]).getByText("Not linked to a library album"),
  ).toBeTruthy();
  expect(
    within(albums[1]).queryByRole("link", { name: "Open in your library" }),
  ).toBeNull();
  expect(http.calls).toEqual({ associations: 1, profile: 1, discography: 1 });
  expect(player.playQueue).not.toHaveBeenCalled();
  expect(player.subscribePosition).not.toHaveBeenCalled();
});

test("a type filter narrows the loaded releases and long sections page locally", async () => {
  const http = artistGroupHttp();
  const albums = Array.from({ length: 14 }, (_, index) =>
    release(`rg:album-${index}`, "album", `Album ${index}`),
  );
  http.reply("discography", page([...albums, release("rg:ep", "ep")], null));
  const region = await open();
  await within(region).findByRole("heading", { name: "Albums (14)" });
  const list = within(
    within(region).getByRole("region", { name: "Albums" }),
  ).getByRole("list");
  expect(list.children).toHaveLength(10);
  fireEvent.click(
    within(region).getByRole("button", { name: "Show more albums" }),
  );
  expect(list.children).toHaveLength(14);
  fireEvent.click(within(region).getByRole("button", { name: "EPs · 1" }));
  expect(within(region).queryByRole("region", { name: "Albums" })).toBeNull();
  expect(within(region).getByRole("region", { name: "EPs" })).toBeTruthy();
  expect(
    within(region)
      .getByRole("button", { name: "EPs · 1" })
      .getAttribute("aria-pressed"),
  ).toBe("true");
});

test("a later page checks only the releases that are new", async () => {
  const http = artistGroupHttp();
  http.reply("discography", page([release("rg:one", "album")], "next"));
  const region = await open();
  await within(region).findByText("Not linked to a library album");
  http.reply(
    "discography",
    page([release("rg:one", "album"), release("rg:two", "single")], null),
  );
  fireEvent.click(
    within(region).getByRole("button", { name: "Load more releases" }),
  );
  await waitFor(() => expect(http.libraryRequests).toHaveLength(2));
  expect(http.libraryRequests).toEqual([["rg:one"], ["rg:two"]]);
});

test("a failed library check keeps the catalog and retries only the availability", async () => {
  const http = artistGroupHttp();
  http.reply("discography", page([release("rg:one", "album")], null));
  http.replyLibrary({ code: "internal", message: "broken" }, 500);
  const region = await open();
  await within(region).findByText(/Library availability could not be checked/);
  expect(
    within(region).getByRole("heading", { name: "Title of rg:one" }),
  ).toBeTruthy();
  expect(http.libraryRequests).toHaveLength(1);
  fireEvent.click(
    within(region).getByRole("button", { name: "Check library again" }),
  );
  await within(region).findByText("Not linked to a library album");
  expect(http.libraryRequests).toHaveLength(2);
  expect(http.calls).toEqual({ associations: 1, profile: 1, discography: 1 });
});

test("a provider outage is retryable while the rest of the artist page works", async () => {
  const http = artistGroupHttp();
  http.reply("discography", { status: "unavailable" });
  const { detail } = artistIdentityBrowsing();
  render(createElement(ArtistIdentityGroupPage, detail));
  fireEvent.click(
    screen.getByRole("button", { name: "Browse full discography" }),
  );
  const region = await screen.findByRole("region", { name: "Discography" });
  expect((await within(region).findByRole("alert")).textContent).toContain(
    "This discography is unavailable. Try again.",
  );
  expect(http.libraryRequests).toEqual([]);
  expect(screen.getByRole("heading", { name: "Libraries" })).toBeTruthy();
  fireEvent.click(within(region).getByRole("button", { name: "Try again" }));
  await within(region).findByRole("heading", { name: /Albums|Other releases/ });
});

test("each release shows one availability state and the missing-only filter hides complete releases", async () => {
  const http = artistGroupHttp();
  http.reply(
    "discography",
    page(
      [
        release("rg:complete", "album", "Complete one"),
        release("rg:partial", "album", "Partial one"),
        release("rg:absent", "ep", "Absent one"),
      ],
      null,
    ),
  );
  http.replyLibrary({
    association_id: artistAssociation.id,
    groups: [
      {
        release_group_id: "rg:complete",
        albums: [
          {
            release_id: "album-a",
            edition_id: "edition:1",
            track_count: 4,
            playable_count: 4,
          },
        ],
      },
      {
        release_group_id: "rg:partial",
        albums: [
          {
            release_id: "album-b",
            edition_id: "edition:2",
            track_count: 10,
            playable_count: 6,
          },
        ],
      },
      { release_group_id: "rg:absent", albums: [] },
    ],
  });
  const region = await open();
  await within(region).findByText("Partial · 6 of 10");
  expect(within(region).getByText("In library")).toBeTruthy();
  expect(within(region).getByText("Catalog only")).toBeTruthy();
  const missing = within(region).getByRole("button", {
    name: "Only what I'm missing",
  });
  expect(missing.getAttribute("aria-pressed")).toBe("false");
  fireEvent.click(missing);
  expect(missing.getAttribute("aria-pressed")).toBe("true");
  expect(
    within(region).queryByRole("heading", { name: "Complete one" }),
  ).toBeNull();
  expect(
    within(region).getByRole("heading", { name: "Partial one" }),
  ).toBeTruthy();
  expect(
    within(region).getByRole("heading", { name: "Absent one" }),
  ).toBeTruthy();
  fireEvent.click(missing);
  expect(
    within(region).getByRole("heading", { name: "Complete one" }),
  ).toBeTruthy();
});

test("the release type control counts every type and keeps the all choice", async () => {
  const http = artistGroupHttp();
  http.reply(
    "discography",
    page(
      [
        release("rg:one", "album"),
        release("rg:two", "album"),
        release("rg:ep", "ep"),
      ],
      null,
    ),
  );
  const region = await open();
  await within(region).findByRole("heading", { name: "Albums (2)" });
  const group = within(region).getByRole("group", { name: "Release types" });
  expect(
    within(group)
      .getAllByRole("button")
      .map((button) => button.textContent),
  ).toEqual(["All · 3", "Albums · 2", "EPs · 1"]);
  fireEvent.click(within(group).getByRole("button", { name: "EPs · 1" }));
  expect(within(region).queryByRole("region", { name: "Albums" })).toBeNull();
  fireEvent.click(within(group).getByRole("button", { name: "All · 3" }));
  expect(within(region).getByRole("region", { name: "Albums" })).toBeTruthy();
});
