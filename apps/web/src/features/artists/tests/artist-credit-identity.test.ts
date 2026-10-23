import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistCreditIdentity from "../components/ArtistCreditIdentity";
import type { components } from "../../../generated";

type Schema = components["schemas"];

const account = "10000000-0000-4000-8000-000000000001";
const credit = { id: "credit-imported", name: "C4 Pedro" };
const group = "70000000-0000-4000-8000-000000000002";
const groupsPath = "/api/v1/artist-identity-groups?q=C4+Pedro&limit=20";
const applyPath = `/api/v1/artists/${credit.id}/artist-identity`;
const groups = {
  items: [
    {
      id: group,
      provider: { kind: "builtin_musicbrainz" },
      provider_artist_id: "owned:artist/alpha",
      name: {
        value: "C4 Pedro",
        recording_id: "r1",
        artist_id: "a1",
        association_id: "s1",
      },
      library_count: 1,
      recording_count: 3,
      release_count: 1,
    },
  ],
  total: 1,
  offset: 0,
  limit: 20,
} satisfies Schema["Page_ArtistIdentityGroup"];

function receipt(
  body: Schema["ApplyArtistIdentityToCredit"],
  counts: Partial<Schema["ArtistIdentityCreditReceipt"]> = {},
): Schema["ArtistIdentityCreditReceipt"] {
  return {
    request_id: body.request_id,
    artist_id: credit.id,
    group_id: group,
    applied: 2,
    unchanged: 0,
    skipped_different: 0,
    skipped_unavailable: 0,
    next_after: null,
    ...counts,
  };
}

afterEach(() => vi.unstubAllGlobals());

async function open() {
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  fireEvent.click(
    screen.getByRole("button", {
      name: "Find reviewed artists for this credit",
    }),
  );
  return screen.findByRole("button", {
    name: "Use this identity for all tracks with this credit: C4 Pedro",
  });
}

test("a reviewed artist is applied only after confirmation and reports counted outcomes", async () => {
  const writes: Schema["ApplyArtistIdentityToCredit"][] = [];
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    if (path === groupsPath) return Response.json(groups);
    if (path === applyPath && options?.method === "POST") {
      const body = JSON.parse(options.body as string);
      writes.push(body);
      return Response.json(
        receipt(body, { applied: 2, unchanged: 1, skipped_different: 1 }),
      );
    }
    throw new Error(
      "Unexpected request outside the owned credit identity flow",
    );
  });
  vi.stubGlobal("fetch", fetch);
  fireEvent.click(await open());
  expect(writes).toHaveLength(0);
  expect(screen.getByRole("group", { name: "Confirm artist identity" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(writes).toHaveLength(0);
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Use this identity for all tracks with this credit: C4 Pedro",
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  const summary = await screen.findByRole("status");
  expect(summary.textContent).toContain("2 tracks now use this artist");
  expect(summary.textContent).toContain("1 already used it");
  expect(summary.textContent).toContain("1 kept a different reviewed artist");
  expect(writes).toHaveLength(1);
  expect(writes[0].group_id).toBe(group);
  expect(writes[0].after ?? null).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
});

test("large credits continue batch by batch from the returned cursor and add the counts", async () => {
  const writes: Schema["ApplyArtistIdentityToCredit"][] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === groupsPath) return Response.json(groups);
      const body = JSON.parse(options?.body as string);
      writes.push(body);
      return Response.json(
        writes.length === 1
          ? receipt(body, { applied: 100, next_after: "recording-100" })
          : receipt(body, { applied: 5, skipped_unavailable: 2 }),
      );
    }),
  );
  fireEvent.click(await open());
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  const summary = await screen.findByRole("status");
  expect(summary.textContent).toContain("105 tracks now use this artist");
  expect(summary.textContent).toContain("2 could not be changed");
  expect(writes.map((body) => body.after ?? null)).toEqual([
    null,
    "recording-100",
  ]);
  expect(writes[0].request_id).not.toBe(writes[1].request_id);
});

test("a lost acknowledgement retries the exact request and an invalid receipt is rejected", async () => {
  const writes: Schema["ApplyArtistIdentityToCredit"][] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === groupsPath) return Response.json(groups);
      const body = JSON.parse(options?.body as string);
      writes.push(body);
      if (writes.length === 1) throw new TypeError("network lost");
      if (writes.length === 2)
        return Response.json(receipt({ ...body, request_id: group }));
      return Response.json(receipt(body));
    }),
  );
  fireEvent.click(await open());
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await screen.findByRole("alert");
  fireEvent.click(
    screen.getByRole("button", {
      name: "Retry the same artist identity request",
    }),
  );
  await waitFor(() => expect(writes).toHaveLength(2));
  await screen.findByRole("alert");
  expect(screen.queryByRole("status")).toBeNull();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Retry the same artist identity request",
    }),
  );
  await screen.findByRole("status");
  expect(new Set(writes.map((body) => body.request_id)).size).toBe(1);
  expect(writes).toHaveLength(3);
});
