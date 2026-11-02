import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { reload } = vi.hoisted(() => ({ reload: vi.fn() }));
vi.mock("@inertiajs/react", () => ({
  router: { reload },
  Link: ({ href, children, ...rest }: { href: string; children: ReactNode }) =>
    createElement("a", { href, ...rest }, children),
}));

import ArtistReviewQueue from "../components/ArtistReviewQueue";

const account = "10000000-0000-4000-8000-000000000001";
const QUEUE = "/api/v1/artist-matching/review-queue?limit=50";
const DISMISSED = "/api/v1/artist-matching/review-queue/dismissed?limit=50";
const NOVA = "44444444-4444-4444-8444-444444444444";
const OTHER = "55555555-5555-4555-8555-555555555555";

type Candidate = {
  provider_artist_id: string;
  name: string;
  kind: string | null;
  disambiguation: string | null;
  country: string | null;
  corroborated_titles: number;
  in_library: boolean;
  origin: "name_search" | "chosen_artist" | "matched_on_other_tracks";
};

function candidate(over: Partial<Candidate> = {}): Candidate {
  return {
    provider_artist_id: NOVA,
    name: "Nova Quartet",
    kind: "group",
    disambiguation: "Lisbon fado band",
    country: "PT",
    corroborated_titles: 1,
    in_library: false,
    origin: "name_search",
    ...over,
  };
}

function item(id = "credit-1", over: Record<string, unknown> = {}) {
  return {
    artist_id: id,
    name: "Nova Quartet",
    undecided_tracks: 4,
    needs_search: false,
    search_recording_id: null,
    candidates: [
      candidate(),
      candidate({
        provider_artist_id: OTHER,
        disambiguation: "Brazilian jazz trio",
        country: "BR",
        kind: "group",
        corroborated_titles: 0,
      }),
    ],
    ...over,
  };
}

function edited() {
  return item("credit-3", {
    name: "nova quartet",
    undecided_tracks: 1,
    candidates: [
      candidate({
        kind: null,
        disambiguation: null,
        country: null,
        corroborated_titles: 0,
        in_library: true,
        origin: "chosen_artist",
      }),
    ],
  });
}

function page(items: unknown[]) {
  return Response.json({ items, total: items.length, offset: 0, limit: 50 });
}

type Posted = { path: string; body: Record<string, unknown> };

function serve(
  reads: (() => Response)[],
  post: (path: string, body: Record<string, unknown>) => Response,
  dismissed: (() => Response)[] = [() => page([])],
) {
  const posted: Posted[] = [];
  let read = 0;
  let readDismissed = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === QUEUE || path.startsWith(`${QUEUE}&offset=`))
        return reads[Math.min(read++, reads.length - 1)]();
      if (path === DISMISSED || path.startsWith(`${DISMISSED}&offset=`))
        return dismissed[Math.min(readDismissed++, dismissed.length - 1)]();
      const body = JSON.parse(options?.body as string);
      posted.push({ path, body });
      return post(path, body);
    }),
  );
  return posted;
}

function show() {
  return render(
    createElement(ArtistReviewQueue, { account, refreshKey: "1-0" }),
  );
}

async function open() {
  const summary = await screen.findByText(/Needs your confirmation/);
  fireEvent.click(summary);
}

beforeEach(() => reload.mockClear());
afterEach(() => vi.unstubAllGlobals());

test("a queue with credits shows a collapsed section with the count", async () => {
  serve([() => page([item(), item("credit-2")])], () => new Response(null));
  const { container } = show();
  const summary = await screen.findByText("Needs your confirmation (2)");
  expect(summary).toBeTruthy();
  const details = container.querySelector("details");
  expect(details?.open).toBe(false);
});

test("nothing is shown for an empty queue or when the queue cannot be read", async () => {
  serve([() => page([])], () => new Response(null));
  const empty = show();
  await waitFor(() => expect(vi.mocked(fetch)).toHaveBeenCalled());
  await Promise.resolve();
  expect(empty.container.textContent).toBe("");
  empty.unmount();

  serve([() => new Response("{}", { status: 500 })], () => new Response(null));
  const failed = show();
  await waitFor(() => expect(vi.mocked(fetch)).toHaveBeenCalled());
  await Promise.resolve();
  expect(failed.container.textContent).toBe("");
});

test("each credit lists its track count and candidates with their details", async () => {
  serve([() => page([item()])], () => new Response(null));
  show();
  await open();
  expect(screen.getByText("Nova Quartet", { selector: "strong" })).toBeTruthy();
  expect(screen.getByText("4 tracks")).toBeTruthy();
  expect(screen.getByText(/Lisbon fado band/)).toBeTruthy();
  expect(screen.getByText(/Brazilian jazz trio/)).toBeTruthy();
  expect(screen.getByText(/Portugal/)).toBeTruthy();
  expect(
    screen.getByRole("button", {
      name: "Confirm Nova Quartet, Lisbon fado band, for Nova Quartet",
    }),
  ).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "None of these for Nova Quartet" }),
  ).toBeTruthy();
});

test("one click on Confirm applies the artist, removes the row and refreshes the chosen artists", async () => {
  const posted = serve([() => page([item()]), () => page([])], () =>
    Response.json({
      request_id: "r",
      artist_id: "credit-1",
      provider_artist_id: NOVA,
      applied: 4,
      kept: 0,
      remaining: 0,
    }),
  );
  show();
  await open();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Confirm Nova Quartet, Lisbon fado band, for Nova Quartet",
    }),
  );
  await waitFor(() => expect(posted.length).toBe(1));
  expect(posted[0].path).toBe(
    "/api/v1/artist-matching/review-queue/credit-1/confirm",
  );
  expect(posted[0].body.provider_artist_id).toBe(NOVA);
  expect(typeof posted[0].body.request_id).toBe("string");
  await waitFor(() =>
    expect(screen.queryByText(/Needs your confirmation/)).toBeNull(),
  );
  expect(reload).toHaveBeenCalledWith({
    only: ["artists", "identity_groups"],
  });
});

test("a confirmation that did not reach every track asks again until none remain", async () => {
  let calls = 0;
  const posted = serve([() => page([item()]), () => page([])], () => {
    calls += 1;
    return Response.json({
      request_id: "r",
      artist_id: "credit-1",
      provider_artist_id: NOVA,
      applied: 200,
      kept: 0,
      remaining: calls === 1 ? 50 : 0,
      next_after: calls === 1 ? "recording-200" : null,
    });
  });
  show();
  await open();
  fireEvent.click(screen.getAllByRole("button", { name: /^Confirm/ })[0]);
  await waitFor(() => expect(posted.length).toBe(2));
  expect(posted[0].body.request_id).not.toBe(posted[1].body.request_id);
  expect(posted[0].body.after ?? null).toBeNull();
  expect(posted[1].body.after).toBe("recording-200");
});

test("a step that fails after earlier progress still shows what was saved", async () => {
  let calls = 0;
  const posted = serve([() => page([item()]), () => page([item()])], () => {
    calls += 1;
    if (calls === 2) return new Response("{}", { status: 500 });
    return Response.json({
      request_id: "r",
      artist_id: "credit-1",
      provider_artist_id: NOVA,
      applied: 200,
      kept: 0,
      remaining: 50,
      next_after: "recording-200",
    });
  });
  show();
  await open();
  fireEvent.click(screen.getAllByRole("button", { name: /^Confirm/ })[0]);
  expect((await screen.findByRole("alert")).textContent).toMatch(
    /could not be saved/i,
  );
  expect(posted.length).toBe(2);
  expect(reload).toHaveBeenCalledWith({
    only: ["artists", "identity_groups"],
  });
  const reads = vi
    .mocked(fetch)
    .mock.calls.filter(([path]) => path === QUEUE).length;
  expect(reads, "the queue is read again after the failure").toBe(2);
});

test("None of these dismisses every candidate of the credit and removes the row", async () => {
  const posted = serve([() => page([item()]), () => page([])], () =>
    Response.json({ artist_id: "credit-1", dismissed: 2 }),
  );
  show();
  await open();
  fireEvent.click(
    screen.getByRole("button", { name: "None of these for Nova Quartet" }),
  );
  await waitFor(() => expect(posted.length).toBe(1));
  expect(posted[0].path).toBe(
    "/api/v1/artist-matching/review-queue/credit-1/dismiss",
  );
  expect(posted[0].body).toEqual({});
  await waitFor(() =>
    expect(screen.queryByText(/Needs your confirmation/)).toBeNull(),
  );
  expect(reload).not.toHaveBeenCalled();
});

test("a failed confirmation keeps the row and says so", async () => {
  serve([() => page([item()])], () => new Response("{}", { status: 409 }));
  show();
  await open();
  fireEvent.click(screen.getAllByRole("button", { name: /^Confirm/ })[0]);
  expect((await screen.findByRole("alert")).textContent).toMatch(
    /could not be saved/i,
  );
  expect(screen.getByText("4 tracks")).toBeTruthy();
  expect(reload).not.toHaveBeenCalled();
});

test("the queue is read again when the artist lists change", async () => {
  const calls = serve([() => page([item()])], () => new Response(null));
  const view = show();
  await screen.findByText("Needs your confirmation (1)");
  const reads = () =>
    vi.mocked(fetch).mock.calls.filter(([path]) => path === QUEUE).length;
  expect(reads()).toBe(1);
  view.rerender(
    createElement(ArtistReviewQueue, { account, refreshKey: "2-1" }),
  );
  await waitFor(() => expect(reads()).toBe(2));
  expect(calls.length).toBe(0);
});

const restored = () => Response.json({ artist_id: "credit-1", restored: 1 });

function dismissedItem(id = "credit-1", over: Record<string, unknown> = {}) {
  return item(id, { candidates: [candidate()], ...over });
}

test("dismissed suggestions stay reachable when nothing else needs confirmation", async () => {
  serve([() => page([])], restored, [() => page([dismissedItem()])]);
  const { container } = show();
  await screen.findByText("Dismissed suggestions (1)");
  expect(screen.queryByText(/Needs your confirmation/)).toBeNull();
  const disclosures = container.querySelectorAll("details");
  expect(disclosures.length).toBe(1);
  expect(disclosures[0].open).toBe(false);
});

test("a dismissed candidate is listed with its details and Restore offers it again", async () => {
  const posted = serve([() => page([]), () => page([item()])], restored, [
    () => page([dismissedItem()]),
    () => page([]),
  ]);
  show();
  fireEvent.click(await screen.findByText("Dismissed suggestions (1)"));
  expect(screen.getByText(/Lisbon fado band/)).toBeTruthy();
  expect(screen.getByText("4 tracks")).toBeTruthy();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Restore Nova Quartet, Lisbon fado band, for Nova Quartet",
    }),
  );
  await waitFor(() => expect(posted.length).toBe(1));
  expect(posted[0].path).toBe(
    "/api/v1/artist-matching/review-queue/credit-1/restore",
  );
  expect(posted[0].body).toEqual({ provider_artist_id: NOVA });
  await screen.findByText("Needs your confirmation (1)");
  expect(screen.queryByText(/Dismissed suggestions/)).toBeNull();
  expect(reload).not.toHaveBeenCalled();
});

test("a credit with several dismissed candidates can restore them all at once", async () => {
  const posted = serve([() => page([])], restored, [
    () => page([item()]),
    () => page([]),
  ]);
  show();
  fireEvent.click(await screen.findByText("Dismissed suggestions (1)"));
  fireEvent.click(
    screen.getByRole("button", { name: "Restore all for Nova Quartet" }),
  );
  await waitFor(() => expect(posted.length).toBe(1));
  expect(posted[0].body).toEqual({});
});

test("a failed restore keeps the row and says so", async () => {
  serve([() => page([])], () => new Response("{}", { status: 500 }), [
    () => page([dismissedItem()]),
  ]);
  show();
  fireEvent.click(await screen.findByText("Dismissed suggestions (1)"));
  fireEvent.click(screen.getAllByRole("button", { name: /^Restore/ })[0]);
  expect((await screen.findByRole("alert")).textContent).toMatch(
    /could not be restored/i,
  );
  expect(screen.getByText("4 tracks")).toBeTruthy();
});

test("a dismissed list that cannot be read leaves the confirmation queue working", async () => {
  serve([() => page([item()])], restored, [
    () => new Response("{}", { status: 500 }),
  ]);
  show();
  await screen.findByText("Needs your confirmation (1)");
  expect(screen.queryByText(/Dismissed suggestions \(/)).toBeNull();
  expect(
    screen.getByText("Dismissed suggestions could not be loaded."),
  ).toBeTruthy();
});

test("an unreadable dismissed list says so even when nothing else needs confirmation", async () => {
  serve([() => page([])], restored, [
    () => new Response("{}", { status: 500 }),
  ]);
  show();
  expect(
    await screen.findByText("Dismissed suggestions could not be loaded."),
  ).toBeTruthy();
});

function pageOf(items: unknown[], total: number, offset = 0) {
  return Response.json({ items, total, offset, limit: 50 });
}

test("dismissed credits past the first page are reachable with Show more", async () => {
  serve([() => page([])], restored, [
    () => pageOf([dismissedItem("c1")], 2),
    () => pageOf([dismissedItem("c2", { name: "Second Credit" })], 2, 1),
  ]);
  show();
  fireEvent.click(await screen.findByText("Dismissed suggestions (2)"));
  expect(screen.queryByText("Second Credit")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Show more dismissed" }));
  await screen.findByText("Second Credit");
  expect(vi.mocked(fetch).mock.calls.map(([path]) => path)).toContain(
    `${DISMISSED}&offset=1`,
  );
  expect(
    screen.queryByRole("button", { name: "Show more dismissed" }),
  ).toBeNull();
});

test("credits past the first page of the queue are reachable with Show more", async () => {
  serve(
    [
      () => pageOf([item("c1")], 2),
      () => pageOf([item("c2", { name: "Second Credit" })], 2, 1),
    ],
    restored,
  );
  show();
  await open();
  fireEvent.click(screen.getByRole("button", { name: "Show more" }));
  await screen.findByText("Second Credit");
  expect(vi.mocked(fetch).mock.calls.map(([path]) => path)).toContain(
    `${QUEUE}&offset=1`,
  );
});

test("a credit without candidates says no match was found and offers a Search link to the artist search", async () => {
  serve(
    [
      () =>
        page([
          item("credit-2", {
            name: "Solo Act",
            undecided_tracks: 2,
            needs_search: true,
            search_recording_id: "rec-9",
            candidates: [],
          }),
        ]),
    ],
    () => new Response(null),
  );
  show();
  await open();
  expect(screen.getByText("No match found automatically")).toBeTruthy();
  const search = screen.getByRole("link", { name: "Search for Solo Act" });
  expect(search.getAttribute("href")).toBe("/recordings/rec-9?review=artist");
  expect(screen.getByText("2 tracks")).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Confirm/ })).toBeNull();
  expect(screen.queryByRole("button", { name: /None of these/ })).toBeNull();
});

test("a personally edited credit offers the artist the account already chose and confirms it in one click", async () => {
  const posted = serve([() => page([edited()]), () => page([])], () =>
    Response.json({
      request_id: "r",
      artist_id: "credit-3",
      provider_artist_id: NOVA,
      applied: 1,
      kept: 0,
      remaining: 0,
    }),
  );
  show();
  await open();
  expect(screen.getByText(/Same artist as Nova Quartet/)).toBeTruthy();
  expect(screen.queryByText("No match found automatically")).toBeNull();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Confirm Nova Quartet, for nova quartet",
    }),
  );
  await waitFor(() => expect(posted.length).toBe(1));
  expect(posted[0].path).toBe(
    "/api/v1/artist-matching/review-queue/credit-3/confirm",
  );
  expect(posted[0].body.provider_artist_id).toBe(NOVA);
  await waitFor(() =>
    expect(screen.queryByText(/Needs your confirmation/)).toBeNull(),
  );
});

test("the same-artist offer can be declined with Not this artist", async () => {
  const posted = serve([() => page([edited()]), () => page([])], () =>
    Response.json({ artist_id: "credit-3", dismissed: 1 }),
  );
  show();
  await open();
  fireEvent.click(
    screen.getByRole("button", { name: "None of these for nova quartet" }),
  );
  await waitFor(() => expect(posted.length).toBe(1));
  expect(posted[0].path).toBe(
    "/api/v1/artist-matching/review-queue/credit-3/dismiss",
  );
});

test("one artist offered both by the name search and as the chosen artist is shown once with both kinds of detail", async () => {
  serve(
    [
      () =>
        page([
          item("credit-4", {
            name: "nova quartet",
            undecided_tracks: 2,
            candidates: [
              candidate({
                origin: "chosen_artist",
                in_library: true,
                corroborated_titles: 2,
              }),
            ],
          }),
        ]),
    ],
    () => new Response(null),
  );
  show();
  await open();
  expect(screen.getAllByRole("button", { name: /^Confirm/ })).toHaveLength(1);
  expect(screen.getByText(/Same artist as Nova Quartet/)).toBeTruthy();
  expect(screen.getByText(/Lisbon fado band/)).toBeTruthy();
  expect(screen.getByText(/2 titles match your library/)).toBeTruthy();
});

test("an artist matched on the credit's other tracks says how many tracks matched and confirms in one click", async () => {
  serve(
    [
      () =>
        page([
          item("credit-5", {
            name: "nova quartet",
            undecided_tracks: 2,
            candidates: [
              candidate({
                origin: "matched_on_other_tracks",
                in_library: true,
                corroborated_titles: 2,
              }),
            ],
          }),
        ]),
    ],
    () => new Response(null),
  );
  show();
  await open();
  expect(
    screen.getByText(/Matched on 2 other tracks of this credit/),
  ).toBeTruthy();
  expect(screen.queryByText(/titles match your library/)).toBeNull();
  expect(screen.getAllByRole("button", { name: /^Confirm/ })).toHaveLength(1);
});
