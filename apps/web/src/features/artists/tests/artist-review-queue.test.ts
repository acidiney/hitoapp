import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { reload } = vi.hoisted(() => ({ reload: vi.fn() }));
vi.mock("@inertiajs/react", () => ({ router: { reload } }));

import ArtistReviewQueue from "../components/ArtistReviewQueue";

const account = "10000000-0000-4000-8000-000000000001";
const QUEUE = "/api/v1/artist-matching/review-queue?limit=50";
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
    ...over,
  };
}

function item(id = "credit-1", over: Record<string, unknown> = {}) {
  return {
    artist_id: id,
    name: "Nova Quartet",
    undecided_tracks: 4,
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

function page(items: unknown[]) {
  return Response.json({ items, total: items.length, offset: 0, limit: 50 });
}

type Posted = { path: string; body: Record<string, unknown> };

function serve(
  reads: (() => Response)[],
  post: (path: string, body: Record<string, unknown>) => Response,
) {
  const posted: Posted[] = [];
  let read = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === QUEUE) return reads[Math.min(read++, reads.length - 1)]();
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
  expect(screen.getByText(/PT/)).toBeTruthy();
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
