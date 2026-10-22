import { createElement, StrictMode, type AnchorHTMLAttributes } from "react";
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
import type { ArtistAssociation } from "../../../shared/types";
import {
  artistAssociation,
  reviewedArtistProfile,
} from "../fixtures/artistReview";
import {
  discographyTitle,
  firstDiscographyPage,
  hostDiscographyContinuation,
  lastDiscographyPage,
  providerDiscographyCursor,
} from "../fixtures/artistDiscography";

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
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
});

test("StrictMode starts only one successful first discography read", async () => {
  const { detail } = artistIdentityBrowsing();
  const http = artistGroupHttp();
  const view = render(
    createElement(
      StrictMode,
      null,
      createElement(ArtistIdentityGroupPage, detail),
    ),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Browse full discography" }),
  );
  const region = await screen.findByRole("region", { name: "Discography" });
  await within(region).findByRole("heading", { name: discographyTitle });
  view.rerender(
    createElement(
      StrictMode,
      null,
      createElement(ArtistIdentityGroupPage, structuredClone(detail)),
    ),
  );
  await waitFor(() => expect(region.getAttribute("aria-busy")).toBe("false"));
  expect(http.calls).toEqual({ associations: 1, profile: 1, discography: 1 });
});

test.each([
  "association",
  "account",
  "recording",
  "artist",
  "provider",
  "provider_connection",
  "provider_artist",
] as const)(
  "a mismatched %s witness cannot browse a namesake",
  async (field) => {
    const { detail } = artistIdentityBrowsing();
    const wrong: ArtistAssociation = structuredClone(artistAssociation);
    switch (field) {
      case "association":
        wrong.id = "other-association";
        break;
      case "account":
        wrong.account_id = "other-account";
        break;
      case "recording":
        wrong.recording_id = "other-recording";
        break;
      case "artist":
        wrong.artist_id = "other-credit";
        break;
      case "provider":
        wrong.provider = { kind: "builtin_musicbrainz" };
        break;
      case "provider_connection":
        wrong.provider = {
          kind: "http_plugin",
          installation_id: "other-connection",
        };
        break;
      case "provider_artist":
        wrong.provider_artist_id = "other-provider-artist";
        break;
    }
    const http = artistGroupHttp();
    http.reply("associations", [wrong]);
    render(createElement(ArtistIdentityGroupPage, detail));
    fireEvent.click(
      screen.getByRole("button", { name: "Browse full discography" }),
    );
    const region = screen.getByRole("region", { name: "Full discography" });
    expect((await within(region).findByRole("alert")).textContent).toContain(
      "This chosen artist has changed.",
    );
    expect(http.calls).toEqual({ associations: 1, profile: 0, discography: 0 });
    expect(within(region).queryByRole("list")).toBeNull();
    expect(mutate).not.toHaveBeenCalled();
    expect(player.playQueue).not.toHaveBeenCalled();
  },
);

test.each(["association", "artist"] as const)(
  "a mismatched profile %s cannot start discography",
  async (field) => {
    const { detail } = artistIdentityBrowsing();
    const wrong = structuredClone(reviewedArtistProfile);
    if (field === "association") wrong.association_id = "other-association";
    else wrong.profile.artist.id = "same-name-different-provider-artist";
    const http = artistGroupHttp();
    http.reply("profile", wrong);
    render(createElement(ArtistIdentityGroupPage, detail));
    fireEvent.click(
      screen.getByRole("button", { name: "Browse full discography" }),
    );
    expect((await screen.findByRole("alert")).textContent).toContain(
      "This chosen artist has changed.",
    );
    expect(http.calls).toEqual({ associations: 1, profile: 1, discography: 0 });
    expect(screen.queryByRole("region", { name: "Discography" })).toBeNull();
  },
);

test.each([
  ["disabled", "Metadata providers are disabled"],
  ["unsupported", "does not support artist profiles"],
  ["unavailable", "Artist information is unavailable"],
  ["cancelled", "request was cancelled"],
] as const)(
  "a %s profile stays honest until an explicit retry",
  async (status, message) => {
    const { detail } = artistIdentityBrowsing();
    const http = artistGroupHttp();
    http.reply("profile", { status });
    render(createElement(ArtistIdentityGroupPage, detail));
    fireEvent.click(
      screen.getByRole("button", { name: "Browse full discography" }),
    );
    expect((await screen.findByRole("alert")).textContent).toContain(message);
    expect(http.calls).toEqual({ associations: 1, profile: 1, discography: 0 });
    expect(
      screen.queryByRole("list", { name: "Discography releases" }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    const region = await screen.findByRole("region", { name: "Discography" });
    await within(region).findByRole("heading", { name: discographyTitle });
    expect(http.calls).toEqual({ associations: 2, profile: 2, discography: 1 });
    expect(mutate).not.toHaveBeenCalled();
  },
);

test("one action browses the chosen artist's exact full discography independently of library pages", async () => {
  const { detail, group } = artistIdentityBrowsing();
  const before = structuredClone(detail);
  const errors = vi.spyOn(console, "error");
  const audio = vi.fn();
  vi.stubGlobal("Audio", audio);
  const associations = `/api/v1/recordings/${group.name.recording_id}/artist-associations`;
  const profilePath = `${associations}/${group.name.association_id}/profile`;
  const discographyPath = `${associations}/${group.name.association_id}/discography`;
  const responses: Response[] = [];
  let pages = 0;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    let value: unknown;
    if (method === "GET" && path === associations)
      value = [
        {
          ...artistAssociation,
          id: "same-name-different-association",
          provider_artist_id: "same-name-different-provider-artist",
        },
        artistAssociation,
      ];
    else if (method === "GET" && path === profilePath)
      value = reviewedArtistProfile;
    else if (method === "POST" && path === discographyPath)
      value = ++pages === 1 ? firstDiscographyPage : lastDiscographyPage;
    else throw new Error("Unexpected request outside chosen artist browsing");
    const response = Response.json(value);
    responses.push(response);
    return response;
  });
  vi.stubGlobal("fetch", fetch);
  const path = `/artist-groups/${group.id}`;
  window.history.replaceState(
    null,
    "",
    `${path}?library=owned-local-library&limit=2&library_offset=0&recording_offset=0&release_offset=0`,
  );
  const view = render(createElement(ArtistIdentityGroupPage, detail));
  expect(fetch).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "Browse full discography" }),
  );
  const region = await screen.findByRole("region", { name: "Discography" });
  const list = await within(region).findByRole("list", {
    name: "Discography releases",
  });
  await waitFor(() =>
    expect(within(list).getAllByRole("listitem")).toHaveLength(1),
  );
  expect(
    within(list).getByRole("heading", { name: discographyTitle }),
  ).toBeTruthy();
  expect(fetch.mock.calls.map(([url]) => url)).toEqual([
    associations,
    profilePath,
    discographyPath,
  ]);
  expect(JSON.parse(fetch.mock.calls[2][1]?.body as string)).toEqual({
    limit: 20,
    continuation: null,
  });
  view.rerender(
    createElement(ArtistIdentityGroupPage, structuredClone(detail)),
  );
  await waitFor(() => expect(region.getAttribute("aria-busy")).toBe("false"));
  expect(fetch).toHaveBeenCalledTimes(3);
  fireEvent.click(
    within(screen.getByRole("navigation", { name: "Album pages" })).getByRole(
      "button",
      { name: "Next" },
    ),
  );
  expect(navigation.get).toHaveBeenLastCalledWith(path, {
    library: "owned-local-library",
    limit: "2",
    library_offset: "0",
    recording_offset: "0",
    release_offset: 2,
  });
  expect(fetch).toHaveBeenCalledTimes(3);
  fireEvent.click(
    within(region).getByRole("button", { name: "Load more releases" }),
  );
  await waitFor(() =>
    expect(within(list).getAllByRole("listitem")).toHaveLength(2),
  );
  expect(
    within(list).getAllByRole("heading", { name: discographyTitle }),
  ).toHaveLength(2);
  expect(fetch).toHaveBeenCalledTimes(4);
  expect(JSON.parse(fetch.mock.calls[3][1]?.body as string)).toEqual({
    limit: 20,
    continuation: hostDiscographyContinuation,
  });
  expect(fetch.mock.calls[3][1]?.body).not.toContain(providerDiscographyCursor);
  for (const [, options] of fetch.mock.calls) {
    expect(options?.credentials).toBe("same-origin");
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      detail.user.id,
    );
  }
  expect(responses.every((response) => response.bodyUsed)).toBe(true);
  expect(
    within(region).queryByRole("button", { name: /play|download|import/i }),
  ).toBeNull();
  expect(detail).toEqual(before);
  expect(audio).not.toHaveBeenCalled();
  expect(player.playQueue).not.toHaveBeenCalled();
  expect(player.subscribePosition).not.toHaveBeenCalled();
  expect(mutate).not.toHaveBeenCalled();
  expect(errors).not.toHaveBeenCalled();
});

test("an unavailable automatic first page waits for an explicit retry", async () => {
  const { detail } = artistIdentityBrowsing();
  const http = artistGroupHttp();
  http.reply("discography", { status: "unavailable" });
  render(createElement(ArtistIdentityGroupPage, detail));
  fireEvent.click(
    screen.getByRole("button", { name: "Browse full discography" }),
  );
  const region = await screen.findByRole("region", { name: "Discography" });
  expect((await within(region).findByRole("alert")).textContent).toContain(
    "This discography is unavailable",
  );
  expect(http.calls).toEqual({ associations: 1, profile: 1, discography: 1 });
  fireEvent.click(within(region).getByRole("button", { name: "Try again" }));
  await within(region).findByRole("heading", { name: discographyTitle });
  expect(http.calls).toEqual({ associations: 1, profile: 1, discography: 2 });
  expect(within(region).getByText("Type not provided")).toBeTruthy();
  expect(within(region).getByText("Artist credit not provided")).toBeTruthy();
  expect(within(region).getByText("Release date not provided")).toBeTruthy();
});
