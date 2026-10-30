import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { reload } = vi.hoisted(() => ({ reload: vi.fn() }));
vi.mock("@inertiajs/react", () => ({
  router: { reload },
  Link: ({ children, href }: { children: React.ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

import ArtistMatchingStatus from "../components/ArtistMatchingStatus";

const account = "10000000-0000-4000-8000-000000000001";
const STATUS = "/api/v1/artist-matching/status";
const RUN = "/api/v1/artist-matching/provider-evidence";
const NOW = Date.UTC(2026, 9, 30, 12, 0, 0);
const POLL_MS = 3000;

type Status = {
  enabled: boolean;
  state: "idle" | "queued" | "running";
  matched_so_far: number;
  last_finished_at: number | null;
  last_matched: number;
};

function status(over: Partial<Status> = {}): Status {
  return {
    enabled: true,
    state: "idle",
    matched_so_far: 0,
    last_finished_at: NOW / 1000 - 300,
    last_matched: 12,
    ...over,
  };
}

function serve(
  statuses: (() => Status)[],
  run?: (body: { request_id: string; after?: string }) => Response,
) {
  const calls: string[] = [];
  let index = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      calls.push(path);
      if (path === STATUS)
        return Response.json(
          statuses[Math.min(index++, statuses.length - 1)](),
        );
      expect(path).toBe(RUN);
      return run!(JSON.parse(options?.body as string));
    }),
  );
  return calls;
}

function receipt(
  body: { request_id: string },
  counts: { matched?: number; uncertain?: number; unavailable?: number },
  next: string | null,
) {
  return Response.json({
    request_id: body.request_id,
    matched: counts.matched ?? 0,
    uncertain: counts.uncertain ?? 0,
    declined: 0,
    unavailable: counts.unavailable ?? 0,
    next_after: next,
  });
}

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

async function open() {
  const view = render(createElement(ArtistMatchingStatus, { account }));
  await advance(0);
  return view;
}

beforeEach(() => {
  reload.mockReset();
  localStorage.clear();
  vi.useFakeTimers({ toFake: ["Date", "setTimeout", "clearTimeout"] });
  vi.setSystemTime(NOW);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    value: "visible",
  });
});

test("an idle server shows how many were matched and when, and the page starts no pass of its own", async () => {
  const calls = serve([() => status()]);
  await open();
  expect(screen.getByText(/12 matched · last run 5 minutes ago/)).toBeTruthy();
  expect(calls).toEqual([STATUS]);
  expect(reload).not.toHaveBeenCalled();
  expect(localStorage.length).toBe(0);
  expect(screen.getByRole("button", { name: "Match again" })).toBeTruthy();
  expect(
    screen.getByRole("link", { name: "Settings" }).getAttribute("href"),
  ).toBe("/settings?section=privacy");
});

test("a server that never finished a run says so instead of inventing a time", async () => {
  serve([() => status({ last_finished_at: null, last_matched: 0 })]);
  await open();
  expect(screen.getByText(/No run yet/)).toBeTruthy();
  expect(screen.queryByText(/last run/)).toBeNull();
});

test("a queued or running walk shows the count so far, polls every few seconds and refreshes the lists once when it ends", async () => {
  const calls = serve([
    () => status({ state: "queued" }),
    () => status({ state: "running", matched_so_far: 3 }),
    () => status({ state: "running", matched_so_far: 7 }),
    () => status({ last_matched: 7 }),
  ]);
  await open();
  expect(screen.getByText(/Matching artists… 0 matched/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Match again" })).toBeNull();
  await advance(POLL_MS);
  expect(screen.getByText(/Matching artists… 3 matched/)).toBeTruthy();
  await advance(POLL_MS);
  expect(screen.getByText(/Matching artists… 7 matched/)).toBeTruthy();
  expect(reload).not.toHaveBeenCalled();
  await advance(POLL_MS);
  expect(screen.getByText(/7 matched · last run/)).toBeTruthy();
  expect(reload).toHaveBeenCalledTimes(1);
  expect(reload).toHaveBeenCalledWith({ only: ["artists", "identity_groups"] });
  expect(calls.filter((path) => path === STATUS)).toHaveLength(4);
});

test("polling stops when the page is left", async () => {
  const calls = serve([() => status({ state: "running" })]);
  const view = await open();
  await advance(POLL_MS);
  const before = calls.length;
  view.unmount();
  await advance(POLL_MS * 5);
  expect(calls).toHaveLength(before);
});

test("polling pauses while the tab is hidden and reads again when it is shown", async () => {
  const calls = serve([() => status({ state: "running" })]);
  await open();
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    value: "hidden",
  });
  document.dispatchEvent(new Event("visibilitychange"));
  const before = calls.length;
  await advance(POLL_MS * 5);
  expect(calls).toHaveLength(before);
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    value: "visible",
  });
  document.dispatchEvent(new Event("visibilitychange"));
  await advance(0);
  expect(calls.length).toBeGreaterThan(before);
});

test("nothing is shown and nothing is polled while automatic matching is off", async () => {
  const calls = serve([() => status({ enabled: false })]);
  const view = await open();
  await advance(POLL_MS * 5);
  expect(view.container.textContent).toBe("");
  expect(calls).toEqual([STATUS]);
});

test("a failed status read shows nothing", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json({ error: "down" }, { status: 500 })),
  );
  const view = await open();
  expect(view.container.textContent).toBe("");
});

test("Match again walks the manual pass cursor, counts matches and refreshes the lists once", async () => {
  const afters: (string | undefined)[] = [];
  let release!: () => void;
  const gate = new Promise<void>((resolve) => (release = resolve));
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === STATUS) return Response.json(status());
      const body = JSON.parse(options?.body as string);
      afters.push(body.after);
      if (!body.after) return receipt(body, { matched: 2 }, "r-8");
      await gate;
      return receipt(body, { matched: 1 }, null);
    }),
  );
  await open();
  fireEvent.click(screen.getByRole("button", { name: "Match again" }));
  await advance(0);
  expect(screen.getByText(/Matching artists… 2 matched/)).toBeTruthy();
  release();
  await advance(0);
  expect(afters).toEqual([undefined, "r-8"]);
  expect(reload).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Match again" })).toBeTruthy();
});

test("an unavailable provider ends a manual pass without retrying and says so", async () => {
  const run = vi.fn((body: { request_id: string }) =>
    receipt(body, { unavailable: 2 }, "r-8"),
  );
  serve([() => status()], run);
  await open();
  fireEvent.click(screen.getByRole("button", { name: "Match again" }));
  await advance(0);
  expect(screen.getByText(/The provider was unavailable/)).toBeTruthy();
  expect(run).toHaveBeenCalledTimes(1);
  expect(reload).not.toHaveBeenCalled();
});

test("a name cursor that does not advance ends the manual walk without looping", async () => {
  const run = vi.fn((body: { request_id: string }) =>
    receipt(body, { uncertain: 1 }, "name:a"),
  );
  serve([() => status()], run);
  await open();
  fireEvent.click(screen.getByRole("button", { name: "Match again" }));
  await advance(0);
  expect(run).toHaveBeenCalledTimes(2);
});

test("leaving the page cancels a manual pass", async () => {
  const signals: AbortSignal[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((path: string, options?: RequestInit) => {
      if (path === STATUS) return Promise.resolve(Response.json(status()));
      signals.push(options!.signal!);
      return new Promise<Response>((_resolve, reject) =>
        options!.signal!.addEventListener("abort", () =>
          reject(new DOMException("aborted", "AbortError")),
        ),
      );
    }),
  );
  const view = await open();
  fireEvent.click(screen.getByRole("button", { name: "Match again" }));
  await advance(0);
  view.unmount();
  expect(signals[0].aborted).toBe(true);
  expect(reload).not.toHaveBeenCalled();
});
