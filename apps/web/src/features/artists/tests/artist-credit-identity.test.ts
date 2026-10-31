import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistCreditIdentity from "../components/ArtistCreditIdentity";
import type { Schema } from "../../../shared/schema";

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
} satisfies Schema<"Page_ArtistIdentityGroup">;

function receipt(
  body: Schema<"ApplyArtistIdentityToCredit">,
  counts: Partial<Schema<"ArtistIdentityCreditReceipt">> = {},
): Schema<"ArtistIdentityCreditReceipt"> {
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
      name: "Choose the artist",
    }),
  );
  return screen.findByRole("button", {
    name: "Use this identity for all tracks with this credit: C4 Pedro",
  });
}

test("a reviewed artist is applied only after confirmation and reports counted outcomes", async () => {
  const writes: Schema<"ApplyArtistIdentityToCredit">[] = [];
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

test("large credits continue batch by batch from the returned cursor, add the counts and finish with one pass from the start", async () => {
  const writes: Schema<"ApplyArtistIdentityToCredit">[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === groupsPath) return Response.json(groups);
      const body = JSON.parse(options?.body as string);
      writes.push(body);
      return Response.json(
        writes.length === 1
          ? receipt(body, { applied: 100, next_after: "recording-100" })
          : writes.length === 2
            ? receipt(body, { applied: 5, skipped_unavailable: 2 })
            : receipt(body, { applied: 1, unchanged: 105 }),
      );
    }),
  );
  fireEvent.click(await open());
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  const summary = await screen.findByRole("status");
  expect(summary.textContent).toContain("106 tracks now use this artist");
  expect(summary.textContent).toContain("2 could not be changed");
  expect(summary.textContent).not.toContain("already used it");
  expect(writes.map((body) => body.after ?? null)).toEqual([
    null,
    "recording-100",
    null,
  ]);
  expect(new Set(writes.map((body) => body.request_id)).size).toBe(3);
});

test("a rejected later batch still reports what earlier batches committed", async () => {
  let calls = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === groupsPath) return Response.json(groups);
      const body = JSON.parse(options?.body as string);
      calls += 1;
      if (calls === 1)
        return Response.json(receipt(body, { applied: 100, next_after: "b" }));
      return Response.json(
        { error: "Changed", code: "conflict" },
        { status: 409 },
      );
    }),
  );
  fireEvent.click(await open());
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await screen.findByRole("alert");
  expect((await screen.findByRole("status")).textContent).toContain(
    "100 tracks now use this artist",
  );
  expect(
    screen.queryByRole("button", {
      name: "Retry the same artist identity request",
    }),
  ).toBeNull();
});

test("a cursor that does not advance stops the walk with an error", async () => {
  const writes: Schema<"ApplyArtistIdentityToCredit">[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === groupsPath) return Response.json(groups);
      const body = JSON.parse(options?.body as string);
      writes.push(body);
      return Response.json(receipt(body, { applied: 1, next_after: "same" }));
    }),
  );
  fireEvent.click(await open());
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await screen.findByRole("alert");
  expect(writes).toHaveLength(2);
  expect(writes[1].after).toBe("same");
});

test.each([408, 429])(
  "a %i response keeps the request identifier for an explicit retry",
  async (status) => {
    const writes: Schema<"ApplyArtistIdentityToCredit">[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (path: string, options?: RequestInit) => {
        if (path === groupsPath) return Response.json(groups);
        const body = JSON.parse(options?.body as string);
        writes.push(body);
        if (writes.length === 1)
          return Response.json(
            { error: "Slow down", code: "busy" },
            { status },
          );
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
    await screen.findByRole("status");
    expect(writes[1].request_id).toBe(writes[0].request_id);
  },
);

test("a request that finishes after the credit changed cannot overwrite the new credit's view", async () => {
  let fail!: () => void;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      if (path === groupsPath) return Response.json(groups);
      return new Promise<Response>((_, reject) => {
        fail = () => reject(new TypeError("network lost"));
      });
    }),
  );
  const view = render(
    createElement(ArtistCreditIdentity, { account, artist: credit }),
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "Choose the artist",
    }),
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Use this identity for all tracks with this credit: C4 Pedro",
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(fail).toBeTypeOf("function"));
  view.rerender(
    createElement(ArtistCreditIdentity, {
      account,
      artist: { id: "credit-other", name: "Another" },
    }),
  );
  fail();
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(
    screen.getByRole("button", {
      name: "Choose the artist",
    }),
  ).toBeTruthy();
});

test("the confirmation says which tracks keep their choice", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json(groups)),
  );
  fireEvent.click(await open());
  const text = screen.getByRole("group", {
    name: "Confirm artist identity",
  }).textContent;
  expect(text).toContain("different reviewed artist");
  expect(text).toContain("needs review");
});

test("only groups reviewed under exactly this credit name are offered", async () => {
  const named = (id: string, value: string) => ({
    ...groups.items[0],
    id,
    name: { ...groups.items[0].name, value },
  });
  vi.stubGlobal(
    "fetch",
    vi.fn(async () =>
      Response.json({
        ...groups,
        items: [
          named("g-royce", "Prince Royce"),
          named("g-prince", "  prince "),
        ],
        total: 2,
      }),
    ),
  );
  render(
    createElement(ArtistCreditIdentity, {
      account,
      artist: { id: "credit-prince", name: "Prince" },
    }),
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "Choose the artist",
    }),
  );
  await screen.findByRole("button", {
    name: "Use this identity for all tracks with this credit: prince",
  });
  expect(screen.queryByText(/Prince Royce/)).toBeNull();
});

test("a lost acknowledgement retries the exact request and an invalid receipt is rejected", async () => {
  const writes: Schema<"ApplyArtistIdentityToCredit">[] = [];
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
