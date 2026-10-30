import type { Schema } from "../../../shared/schema";
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
  kind: Schema<"CatalogReleaseGroup">["kind"],
): Schema<"CatalogReleaseGroup"> {
  return {
    id,
    title: `Title of ${id}`,
    artist_credit: null,
    kind,
    secondary_types: [],
    first_release_date: null,
    revision: null,
  };
}

function page(
  items: Schema<"CatalogReleaseGroup">[],
  continuation: string | null,
) {
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

function albums(from: number, count: number) {
  return Array.from({ length: count }, (_, index) =>
    release(`rg:album-${from + index}`, "album"),
  );
}

async function open() {
  const { detail } = artistIdentityBrowsing();
  render(createElement(ArtistIdentityGroupPage, detail));
  return screen.findByRole("region", { name: "Discography" });
}

function section(region: HTMLElement, name: string) {
  return within(region).getByRole("region", { name });
}

test("show more reveals loaded releases first and then pages only that type from the provider", async () => {
  const http = artistGroupHttp();
  http.reply(
    "discography",
    page([...albums(0, 12), release("rg:ep-0", "ep")], "whole-next"),
  );
  http.reply("discography", page(albums(10, 20), "album-next"));
  http.reply("discography", page(albums(30, 3), null));
  const region = await open();
  await within(region).findByRole("heading", { name: "Albums (12)" });
  const list = within(section(region, "Albums")).getByRole("list");
  expect(list.children).toHaveLength(10);

  fireEvent.click(
    within(region).getByRole("button", { name: "Show more albums" }),
  );
  expect(list.children).toHaveLength(12);
  expect(http.discographyBodies).toHaveLength(1);

  fireEvent.click(
    within(region).getByRole("button", { name: "Show more albums" }),
  );
  await waitFor(() => expect(http.discographyBodies).toHaveLength(2));
  expect(http.discographyBodies[1]).toEqual({
    limit: 20,
    continuation: null,
    release_type: "album",
  });
  await within(region).findByRole("heading", { name: "Albums (30)" });
  expect(list.children).toHaveLength(30);
  expect(within(region).getByRole("heading", { name: "EPs (1)" })).toBeTruthy();

  fireEvent.click(
    within(region).getByRole("button", { name: "Show more albums" }),
  );
  await waitFor(() => expect(http.discographyBodies).toHaveLength(3));
  expect(http.discographyBodies[2]).toEqual({
    limit: 20,
    continuation: "album-next",
    release_type: "album",
  });
  await within(region).findByRole("heading", { name: "Albums (33)" });
  expect(
    within(region).queryByRole("button", { name: "Show more albums" }),
  ).toBeNull();
  expect(
    within(region).getByRole("button", { name: "Load more releases" }),
  ).toBeTruthy();
  expect(http.discographyBodies[0]).toEqual({
    limit: 20,
    continuation: null,
  });
  await waitFor(() =>
    expect(http.libraryRequests.flat()).toContain("rg:album-32"),
  );
  expect(
    http.libraryRequests.flat().filter((id) => id === "rg:album-5"),
  ).toHaveLength(1);
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("a fully loaded catalog never asks the provider to page a type", async () => {
  const http = artistGroupHttp();
  http.reply("discography", page(albums(0, 12), null));
  const region = await open();
  await within(region).findByRole("heading", { name: "Albums (12)" });
  fireEvent.click(
    within(region).getByRole("button", { name: "Show more albums" }),
  );
  expect(
    within(region).queryByRole("button", { name: "Show more albums" }),
  ).toBeNull();
  expect(http.discographyBodies).toHaveLength(1);
});

test("a provider without the filter falls back to the local filter and says so once", async () => {
  const http = artistGroupHttp();
  http.reply("discography", page(albums(0, 4), "whole-next"));
  http.reply("discography", { status: "type_filter_unsupported" });
  const region = await open();
  await within(region).findByRole("heading", { name: "Albums (4)" });
  fireEvent.click(
    within(region).getByRole("button", { name: "Show more albums" }),
  );
  await within(region).findByText(/cannot page one release type/);
  expect(
    within(region).queryByRole("button", { name: "Show more albums" }),
  ).toBeNull();
  expect(http.discographyBodies).toHaveLength(2);
  http.reply("discography", page(albums(4, 2), null));
  fireEvent.click(
    within(region).getByRole("button", { name: "Load more releases" }),
  );
  await within(region).findByRole("heading", { name: "Albums (6)" });
  expect(http.discographyBodies).toHaveLength(3);
  expect(http.discographyBodies[2]).toEqual({
    limit: 20,
    continuation: "whole-next",
  });
});

test("a typed outage keeps the known releases and retries only that type", async () => {
  const http = artistGroupHttp();
  http.reply(
    "discography",
    page([...albums(0, 3), release("rg:single-0", "single")], "whole-next"),
  );
  http.reply("discography", { status: "unavailable" });
  http.reply("discography", page(albums(3, 2), null));
  const region = await open();
  await within(region).findByRole("heading", { name: "Albums (3)" });
  fireEvent.click(
    within(region).getByRole("button", { name: "Show more albums" }),
  );
  expect(
    (await within(section(region, "Albums")).findByRole("alert")).textContent,
  ).toContain("This discography is unavailable. Try again.");
  expect(
    within(region).getByRole("heading", { name: "Albums (3)" }),
  ).toBeTruthy();
  expect(
    within(region).getByRole("heading", { name: "Singles (1)" }),
  ).toBeTruthy();
  expect(
    within(region).getByRole("button", { name: "Load more releases" }),
  ).toBeTruthy();
  fireEvent.click(
    within(section(region, "Albums")).getByRole("button", {
      name: "Try again",
    }),
  );
  await waitFor(() => expect(http.discographyBodies.length).toBe(3));
  await within(region).findByRole("heading", { name: "Albums (5)" });
  expect(http.discographyBodies.slice(1)).toEqual([
    { limit: 20, continuation: null, release_type: "album" },
    { limit: 20, continuation: null, release_type: "album" },
  ]);
});

test("a retired typed continuation restarts that type without losing known releases", async () => {
  const http = artistGroupHttp();
  http.reply("discography", page(albums(0, 2), "whole-next"));
  http.reply("discography", page(albums(2, 2), "album-next"));
  http.reply(
    "discography",
    {
      code: "conflict",
      message: "The catalog continuation changed. Start again.",
    },
    409,
  );
  http.reply("discography", page(albums(2, 3), null));
  const region = await open();
  await within(region).findByRole("heading", { name: "Albums (2)" });
  fireEvent.click(
    within(region).getByRole("button", { name: "Show more albums" }),
  );
  await within(region).findByRole("heading", { name: "Albums (4)" });
  fireEvent.click(
    within(region).getByRole("button", { name: "Show more albums" }),
  );
  await within(section(region, "Albums")).findByRole("alert");
  expect(
    within(region).getByRole("heading", { name: "Albums (4)" }),
  ).toBeTruthy();
  fireEvent.click(
    within(section(region, "Albums")).getByRole("button", {
      name: "Restart albums",
    }),
  );
  await within(region).findByRole("heading", { name: "Albums (5)" });
  expect(http.discographyBodies[3]).toEqual({
    limit: 20,
    continuation: null,
    release_type: "album",
  });
});

test("leaving the page cancels a held typed request", async () => {
  const http = artistGroupHttp();
  http.reply("discography", page(albums(0, 2), "whole-next"));
  const held = http.hold("discography", "headers", page(albums(2, 2), null));
  const region = await open();
  await within(region).findByRole("heading", { name: "Albums (2)" });
  fireEvent.click(
    within(region).getByRole("button", { name: "Show more albums" }),
  );
  await held.entered;
  expect(held.signal()?.aborted).toBe(false);
  cleanup();
  expect(held.signal()?.aborted).toBe(true);
  held.release();
});

test("a type with no loaded releases can still be paged from the provider", async () => {
  const http = artistGroupHttp();
  http.reply("discography", page(albums(0, 3), "whole-next"));
  http.reply("discography", page([release("rg:single-0", "single")], null));
  http.reply("discography", page([], null));
  const region = await open();
  await within(region).findByRole("heading", { name: "Albums (3)" });
  expect(
    within(region).queryByRole("region", { name: "Singles" }),
  ).toBeTruthy();
  fireEvent.click(within(region).getByRole("button", { name: "Show singles" }));
  await within(region).findByRole("heading", { name: "Singles (1)" });
  expect(http.discographyBodies[1]).toEqual({
    limit: 20,
    continuation: null,
    release_type: "single",
  });
  fireEvent.click(within(region).getByRole("button", { name: "Show EPs" }));
  await waitFor(() => expect(http.discographyBodies).toHaveLength(3));
  await waitFor(() =>
    expect(within(region).queryByRole("region", { name: "EPs" })).toBeNull(),
  );
});

test("empty type sections are hidden when the catalog is complete or the filter is unsupported", async () => {
  const complete = artistGroupHttp();
  complete.reply("discography", page(albums(0, 3), null));
  const region = await open();
  await within(region).findByRole("heading", { name: "Albums (3)" });
  expect(within(region).queryByRole("region", { name: "Singles" })).toBeNull();
  cleanup();

  const http = artistGroupHttp();
  http.reply("discography", page(albums(0, 3), "whole-next"));
  http.reply("discography", { status: "type_filter_unsupported" });
  const second = await open();
  await within(second).findByRole("heading", { name: "Albums (3)" });
  fireEvent.click(within(second).getByRole("button", { name: "Show singles" }));
  await within(second).findByText(/cannot page one release type/);
  expect(within(second).queryByRole("region", { name: "Singles" })).toBeNull();
});
