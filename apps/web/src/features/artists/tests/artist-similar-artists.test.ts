import { createElement, StrictMode, type AnchorHTMLAttributes } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import ArtistIdentityGroupPage from "../pages/ArtistIdentityGroupPage";
import { artistIdentityBrowsing } from "../fixtures/artistIdentityBrowsing";

const navigation = vi.hoisted(() => ({ get: vi.fn() }));
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
  router: navigation,
  usePage: () => ({ url: window.location.pathname + window.location.search }),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const MBID = "11111111-1111-4111-8111-111111111111";
const CHOSEN = "22222222-2222-4222-8222-222222222222";
const NAMESAKE = "33333333-3333-4333-8333-333333333333";

function props(provider: "builtin" | "plugin" = "builtin") {
  const { detail } = artistIdentityBrowsing();
  if (provider === "builtin")
    detail.detail.group = {
      ...detail.detail.group,
      provider: { kind: "builtin_musicbrainz" },
      provider_artist_id: MBID,
    };
  return detail;
}

function available(artists: unknown[]) {
  return {
    status: "available",
    association_id: "association",
    provider: { kind: "http_plugin", installation_id: "plugin" },
    artists,
  };
}

type Call = { path: string; init: RequestInit };
function stub(...replies: Array<unknown | (() => Promise<Response>)>) {
  const calls: Call[] = [];
  const fetch = vi.fn(async (path: string, init: RequestInit) => {
    calls.push({ path, init });
    const next = replies.shift();
    if (typeof next === "function") return (next as () => Promise<Response>)();
    return Response.json(next);
  });
  vi.stubGlobal("fetch", fetch);
  return { calls, fetch };
}

beforeEach(() => {
  window.history.replaceState(null, "", "/artist-groups/group");
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const button = () =>
  screen.getByRole("button", { name: "Show similar artists" });

test("the section waits for an explicit request and is absent for other identity providers", () => {
  const http = stub();
  render(createElement(ArtistIdentityGroupPage, props()));
  expect(screen.getByRole("heading", { name: "Similar artists" })).toBeTruthy();
  expect(button()).toBeTruthy();
  expect(http.fetch).not.toHaveBeenCalled();
  cleanup();
  render(createElement(ArtistIdentityGroupPage, props("plugin")));
  expect(screen.queryByRole("heading", { name: "Similar artists" })).toBeNull();
  expect(http.fetch).not.toHaveBeenCalled();
});

test("similar artists link only exact chosen artists and never by name", async () => {
  const shown = props();
  const http = stub(
    available([
      {
        provider_artist_id: CHOSEN,
        name: "Chosen band",
        score: 0.8,
        chosen_artist_id: "chosen-group",
      },
      {
        provider_artist_id: NAMESAKE,
        name: shown.detail.group.name.value,
        score: null,
        chosen_artist_id: null,
      },
    ]),
  );
  render(createElement(ArtistIdentityGroupPage, shown));
  fireEvent.click(button());
  const link = await screen.findByRole("link", { name: "Chosen band" });
  expect(link.getAttribute("href")).toBe("/artist-groups/chosen-group");
  const list = screen.getByRole("list", { name: "Similar artists" });
  expect(list.querySelectorAll("a")).toHaveLength(1);
  expect(list.textContent).toContain("Not one of your chosen artists");
  expect(http.calls).toHaveLength(1);
  const { path, init } = http.calls[0];
  const group = shown.detail.group;
  expect(path).toBe(
    `/api/v1/recordings/${encodeURIComponent(group.name.recording_id)}/artist-associations/${encodeURIComponent(group.name.association_id)}/similar-artists`,
  );
  expect(init.method).toBe("POST");
  expect(JSON.parse(init.body as string)).toEqual({ limit: 10 });
  expect(
    screen.queryByRole("button", { name: "Show similar artists" }),
  ).toBeNull();
});

test.each([
  ["unsupported", /Choose a provider that offers similar artists/],
  ["disabled", /Metadata providers are disabled/],
  ["cancelled", /request was cancelled/],
])("a %s answer is explained with a way forward", async (status, text) => {
  stub({ status });
  render(createElement(ArtistIdentityGroupPage, props()));
  fireEvent.click(button());
  await screen.findByText(text);
  if (status === "unsupported")
    expect(
      screen
        .getByRole("link", { name: "Manage metadata providers" })
        .getAttribute("href"),
    ).toBe("/settings?section=metadata");
});

test("an empty answer is honest", async () => {
  stub(available([]));
  render(createElement(ArtistIdentityGroupPage, props()));
  fireEvent.click(button());
  await screen.findByText(/returned no similar artists/);
});

test("a failing service leaves the rest of the page working and can be retried", async () => {
  const http = stub(
    { status: "unavailable" },
    async () => Response.json({ error: "Bad gateway" }, { status: 502 }),
    available([
      {
        provider_artist_id: CHOSEN,
        name: "Recovered",
        score: null,
        chosen_artist_id: null,
      },
    ]),
  );
  render(createElement(ArtistIdentityGroupPage, props()));
  fireEvent.click(button());
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("unavailable");
  expect(screen.getByRole("heading", { name: "Libraries" })).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "Browse full discography" }),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await waitFor(() =>
    expect(screen.getByRole("alert").textContent).toContain("Bad gateway"),
  );
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByText(/Recovered/)).toBeTruthy();
  expect(http.calls).toHaveLength(3);
  expect(screen.queryByRole("alert")).toBeNull();
});

test("StrictMode and double clicks send one request", async () => {
  const http = stub(available([]));
  render(
    createElement(
      StrictMode,
      null,
      createElement(ArtistIdentityGroupPage, props()),
    ),
  );
  const first = button();
  fireEvent.click(first);
  fireEvent.click(first);
  await screen.findByRole("status", { name: "Similar artists result" });
  expect(http.calls).toHaveLength(1);
});

test("leaving the page cancels the read and a late answer is never shown", async () => {
  let signal: AbortSignal | undefined;
  let answer!: (response: Response) => void;
  vi.stubGlobal(
    "fetch",
    vi.fn(
      (_path: string, init: RequestInit) =>
        new Promise<Response>((resolve, reject) => {
          signal = init.signal as AbortSignal;
          answer = resolve;
          signal.addEventListener("abort", () => reject(signal?.reason));
        }),
    ),
  );
  const view = render(createElement(ArtistIdentityGroupPage, props()));
  fireEvent.click(button());
  await waitFor(() => expect(signal).toBeDefined());
  expect(
    screen.getByRole("status", { name: "Similar artists result" }).textContent,
  ).toContain("Looking");
  view.unmount();
  expect(signal?.aborted).toBe(true);
  answer(Response.json(available([])));
  await Promise.resolve();
  expect(screen.queryByText(/similar artists/i)).toBeNull();
});

test("a read that never answers times out with a retry", async () => {
  vi.useFakeTimers();
  vi.stubGlobal(
    "fetch",
    vi.fn(
      (_path: string, init: RequestInit) =>
        new Promise<Response>((_resolve, reject) => {
          init.signal?.addEventListener("abort", () =>
            reject(init.signal?.reason),
          );
        }),
    ),
  );
  render(createElement(ArtistIdentityGroupPage, props()));
  fireEvent.click(button());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(20_001);
  });
  expect(screen.getByRole("alert").textContent).toContain("timed out");
  expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
});
