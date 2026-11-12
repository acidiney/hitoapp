import { createElement, type AnchorHTMLAttributes } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistCreditIdentity from "../components/ArtistCreditIdentity";
import type { Schema } from "../../../shared/schema";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
  router: { reload: vi.fn(), visit: vi.fn() },
}));

const account = "10000000-0000-4000-8000-000000000001";
const credit = { id: "credit-one", name: "C4 Pedro" };
const strong = "70000000-0000-4000-8000-000000000001";
const weak = "70000000-0000-4000-8000-000000000002";
const suggestionsPath = `/api/v1/artists/${credit.id}/match-suggestions`;
const dismissPath = `${suggestionsPath}/dismiss`;
const restorePath = `${suggestionsPath}/restore`;
const applyPath = `/api/v1/artists/${credit.id}/artist-identity`;

function group(id: string, artist: string): Schema<"ArtistIdentityGroup"> {
  return {
    id,
    provider: { kind: "builtin_musicbrainz" },
    provider_artist_id: artist,
    name: {
      value: "C4 Pedro",
      recording_id: "r1",
      artist_id: "a1",
      association_id: "s1",
    },
    library_count: 1,
    recording_count: 3,
    release_count: 1,
  };
}

const both: Schema<"ArtistMatchSuggestions"> = {
  artist_id: credit.id,
  chosen: [],
  undecided_tracks: 4,
  dismissed: 0,
  dismissed_items: [],
  items: [
    {
      group: group(strong, "11111111-1111-4111-8111-111111111111"),
      shared_albums: 2,
      same_length_tracks: 1,
      undecided_tracks: 4,
    },
    {
      group: group(weak, "22222222-2222-4222-8222-222222222222"),
      shared_albums: 0,
      same_length_tracks: 0,
      undecided_tracks: 4,
    },
  ],
};

afterEach(() => vi.unstubAllGlobals());

test("suggestions explain their evidence, change nothing, and are accepted only through the reviewed apply", async () => {
  const writes: unknown[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === suggestionsPath)
        return Response.json(writes.length ? { ...both, items: [] } : both);
      if (path === applyPath && options?.method === "POST") {
        const body = JSON.parse(options.body as string);
        writes.push(body);
        return Response.json({
          request_id: body.request_id,
          artist_id: credit.id,
          group_id: body.group_id,
          applied: 4,
          unchanged: 0,
          skipped_different: 0,
          skipped_unavailable: 0,
          next_after: null,
        });
      }
      throw new Error("Unexpected request outside the suggestion flow");
    }),
  );
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose the artist" }),
  );
  const list = await screen.findByRole("group", {
    name: /^Which artist is /,
  });
  expect(list.textContent).toContain("2 shared albums");
  expect(list.textContent).toContain("1 track with the same title and length");
  expect(list.textContent).toContain("same name only");
  expect(
    screen.getByRole("dialog", { name: /^Which artist is / }).textContent,
  ).toContain("Nothing changes until you confirm");
  expect(screen.getAllByRole("radio")[0]).toHaveProperty("checked", true);
  expect(writes).toHaveLength(0);

  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  expect(screen.getByRole("group", { name: "Confirm artist identity" }));
  expect(writes).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  const summary = await screen.findByRole("status");
  expect(summary.textContent).toContain("4 tracks now use this artist");
  expect(writes).toEqual([expect.objectContaining({ group_id: strong })]);
  await waitFor(() =>
    expect(
      screen.queryByRole("group", { name: /^Which artist is / }),
    ).toBeNull(),
  );
});

test("dismissing posts the chosen artist, hides the suggestion and keeps the others", async () => {
  const dismissals: unknown[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === suggestionsPath) return Response.json(both);
      if (path === dismissPath && options?.method === "POST") {
        dismissals.push(JSON.parse(options.body as string));
        return Response.json({
          ...both,
          dismissed: 1,
          dismissed_items: both.items.slice(0, 1),
          items: both.items.slice(1),
        });
      }
      throw new Error("Unexpected request outside the suggestion flow");
    }),
  );
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose the artist" }),
  );
  await screen.findByRole("group", { name: /^Which artist is / });
  fireEvent.click(screen.getByRole("radio", { name: /None of these/ }));
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  await waitFor(() => expect(dismissals).toHaveLength(2));
  expect(dismissals).toEqual([
    { group_id: strong },
    { group_id: both.items[1].group.id },
  ]);
});

test("a failed or empty suggestion read leaves the manual flow alone", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      if (path === suggestionsPath) return new Response("{}", { status: 503 });
      if (path.startsWith("/api/v1/artist-identity-groups"))
        return Response.json({ items: [], total: 0, offset: 0, limit: 20 });
      throw new Error("Unexpected request");
    }),
  );
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose the artist" }),
  );
  await screen.findByRole("dialog", { name: /^Which artist is / });
  expect(
    screen.queryByRole("region", { name: "Suggested artists for this credit" }),
  ).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
});

test("a dismissed suggestion is listed in a disclosure and restored on request", async () => {
  const restores: unknown[] = [];
  const dismissedView = {
    ...both,
    dismissed: 1,
    dismissed_items: both.items.slice(0, 1),
    items: both.items.slice(1),
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === suggestionsPath) return Response.json(dismissedView);
      if (path === restorePath && options?.method === "POST") {
        restores.push(JSON.parse(options.body as string));
        return Response.json(both);
      }
      throw new Error("Unexpected request outside the suggestion flow");
    }),
  );
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose the artist" }),
  );
  const list = await screen.findByRole("region", {
    name: "Suggested artists for this credit",
  });
  const disclosure = list.querySelector("details");
  expect(disclosure?.querySelector("summary")?.textContent).toBe(
    "Dismissed suggestions (1)",
  );
  expect(disclosure?.textContent).toContain("2 shared albums");
  expect(restores).toHaveLength(0);
  fireEvent.click(
    screen.getByRole("button", { name: /Restore suggested artist/ }),
  );
  await waitFor(() =>
    expect(
      screen.getAllByRole("radio", { name: /You reviewed this artist/ }),
    ).toHaveLength(2),
  );
  expect(restores).toEqual([{ group_id: strong }]);
  expect(screen.queryByText("Dismissed suggestions (1)")).toBeNull();
});

test("a credit whose only suggestions are dismissed still offers to restore them", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      if (path === suggestionsPath)
        return Response.json({
          ...both,
          dismissed: 2,
          dismissed_items: both.items,
          items: [],
        });
      throw new Error("Unexpected request outside the suggestion flow");
    }),
  );
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose the artist" }),
  );
  await screen.findByText("Dismissed suggestions (2)");
  expect(screen.queryByText("Looks like an artist you reviewed")).toBeNull();
  const region = screen.getByRole("region", {
    name: "Suggested artists for this credit",
  });
  expect(region.querySelectorAll("ul")).toHaveLength(1);
  expect(region.querySelector("details ul")).not.toBeNull();
  expect(region.textContent?.match(/dismissed/gi)).toHaveLength(1);
  expect(
    screen.getAllByRole("button", { name: /Restore suggested/ }),
  ).toHaveLength(2);
  expect(screen.queryByRole("button", { name: /Review suggested/ })).toBeNull();
});
