import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { reload } = vi.hoisted(() => ({ reload: vi.fn() }));
vi.mock("@inertiajs/react", () => ({
  router: { reload },
  Link: ({ children, href }: { children: React.ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

import ProviderArtistMatching from "../components/ProviderArtistMatching";

const account = "10000000-0000-4000-8000-000000000001";
const SETTINGS = "/api/v1/metadata/settings";
const RUN = "/api/v1/artist-matching/provider-evidence";

function settings(enabled: boolean, builtin = true) {
  return Response.json({
    enabled,
    revision: 1,
    providers: builtin ? [{ kind: "builtin_musicbrainz" }] : [],
  });
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

function stub(
  policy: () => Response,
  run: (body: { request_id: string; after?: string }) => Response,
) {
  const calls: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      calls.push(path);
      if (path === SETTINGS) return policy();
      expect(path).toBe(RUN);
      return run(JSON.parse(options?.body as string));
    }),
  );
  return calls;
}

const again = () => screen.getByRole("button", { name: "Match again" });

beforeEach(() => reload.mockReset());
afterEach(() => vi.unstubAllGlobals());

test("opening the page starts the pass by itself and shows one muted status line", async () => {
  const calls = stub(
    () => settings(true),
    (body) => receipt(body, { matched: 16 }, null),
  );
  render(createElement(ProviderArtistMatching, { account }));
  expect(await screen.findByText("16 matched.")).toBeTruthy();
  expect(calls).toEqual([SETTINGS, RUN]);
  expect(screen.queryByRole("heading")).toBeNull();
  expect(screen.queryByText(/Find matches on MusicBrainz/)).toBeNull();
  expect(reload).toHaveBeenCalledTimes(1);
  expect(reload).toHaveBeenCalledWith({ only: ["artists", "identity_groups"] });
});

test("the cursor is walked automatically and the running line counts matches", async () => {
  const afters: (string | undefined)[] = [];
  let release!: () => void;
  const gate = new Promise<void>((resolve) => (release = resolve));
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      if (path === SETTINGS) return settings(true);
      const body = JSON.parse(options?.body as string);
      afters.push(body.after);
      if (!body.after)
        return receipt(body, { matched: 2, uncertain: 1 }, "r-8");
      await gate;
      return receipt(body, { matched: 1, uncertain: 2 }, null);
    }),
  );
  render(createElement(ProviderArtistMatching, { account }));
  expect(await screen.findByText("Matching artists… 2 matched")).toBeTruthy();
  release();
  expect(await screen.findByText("3 matched.")).toBeTruthy();
  expect(afters).toEqual([undefined, "r-8"]);
  expect(reload).toHaveBeenCalledTimes(1);
});

test("an unavailable provider ends the pass without retrying and offers Match again", async () => {
  const run = vi.fn((body: { request_id: string }) =>
    receipt(body, { unavailable: 2 }, "r-8"),
  );
  stub(() => settings(true), run);
  render(createElement(ProviderArtistMatching, { account }));
  expect(
    await screen.findByText("0 matched so far. The provider was unavailable."),
  ).toBeTruthy();
  expect(run).toHaveBeenCalledTimes(1);
  expect(reload).not.toHaveBeenCalled();
  expect(again()).toBeTruthy();
});

test("nothing is sent and nothing is shown when metadata enrichment is off or the built-in provider is not selected", async () => {
  for (const policy of [() => settings(false), () => settings(true, false)]) {
    const calls = stub(policy, (body) => receipt(body, {}, null));
    const view = render(createElement(ProviderArtistMatching, { account }));
    await waitFor(() => expect(calls).toEqual([SETTINGS]));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(calls).toEqual([SETTINGS]);
    expect(view.container.textContent).toBe("");
    view.unmount();
  }
});

test("a pass that ran a moment ago is not repeated on the next visit but can be run again", async () => {
  const calls = stub(
    () => settings(true),
    (body) => receipt(body, { matched: 1 }, null),
  );
  const first = render(createElement(ProviderArtistMatching, { account }));
  await screen.findByText("1 matched.");
  first.unmount();
  calls.length = 0;
  render(createElement(ProviderArtistMatching, { account }));
  await waitFor(() => expect(calls).toEqual([SETTINGS]));
  fireEvent.click(await screen.findByRole("button", { name: "Match again" }));
  await waitFor(() => expect(calls).toEqual([SETTINGS, RUN]));
  expect(await screen.findByText("1 matched.")).toBeTruthy();
});

test("leaving the page cancels a running pass", async () => {
  const signals: AbortSignal[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((path: string, options?: RequestInit) => {
      if (path === SETTINGS) return Promise.resolve(settings(true));
      signals.push(options!.signal!);
      return new Promise<Response>((_resolve, reject) =>
        options!.signal!.addEventListener("abort", () =>
          reject(new DOMException("aborted", "AbortError")),
        ),
      );
    }),
  );
  const view = render(createElement(ProviderArtistMatching, { account }));
  await waitFor(() => expect(signals).toHaveLength(1));
  view.unmount();
  expect(signals[0].aborted).toBe(true);
  expect(reload).not.toHaveBeenCalled();
});

test("the walk continues from the evidence cursor into the name cursor until the server ends it", async () => {
  const afters: (string | undefined)[] = [];
  stub(
    () => settings(true),
    (body) => {
      afters.push(body.after);
      if (!body.after) return receipt(body, { matched: 1 }, "r-8");
      if (body.after === "r-8") return receipt(body, { uncertain: 1 }, "name:");
      if (body.after === "name:")
        return receipt(body, { matched: 2 }, "name:artist-1");
      return receipt(body, { uncertain: 3 }, null);
    },
  );
  render(createElement(ProviderArtistMatching, { account }));
  await screen.findByText("3 matched.");
  expect(afters).toEqual([undefined, "r-8", "name:", "name:artist-1"]);
  expect(reload).toHaveBeenCalledTimes(1);
});

test("a name cursor that does not advance ends the walk without looping", async () => {
  const run = vi.fn((body: { request_id: string; after?: string }) =>
    receipt(body, { uncertain: 1 }, "name:a"),
  );
  stub(() => settings(true), run);
  render(createElement(ProviderArtistMatching, { account }));
  await screen.findByText("0 matched so far.");
  expect(run).toHaveBeenCalledTimes(2);
  expect(again()).toBeTruthy();
});

const pauseKey = `hito:artist-provider-match:${account}`;

test("a cancelled or failed pass does not start the ten minute pause", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn((path: string) =>
      path === SETTINGS
        ? Promise.resolve(settings(true))
        : Promise.reject(new Error("down")),
    ),
  );
  const view = render(createElement(ProviderArtistMatching, { account }));
  await screen.findByText("Matching artists stopped.");
  expect(localStorage.getItem(pauseKey)).toBeNull();
  view.unmount();
});

test("a pass that completes starts the pause", async () => {
  stub(
    () => settings(true),
    (body) => receipt(body, { matched: 1 }, null),
  );
  render(createElement(ProviderArtistMatching, { account }));
  await screen.findByText("1 matched.");
  expect(localStorage.getItem(pauseKey)).not.toBeNull();
});

test("the status line tells where the data goes and links to the setting", async () => {
  stub(
    () => settings(true),
    (body) => receipt(body, { matched: 1 }, null),
  );
  render(createElement(ProviderArtistMatching, { account }));
  await screen.findByText("1 matched.");
  expect(screen.getByText(/Looks up artists on MusicBrainz/)).toBeTruthy();
  expect(
    screen.getByRole("link", { name: "Settings" }).getAttribute("href"),
  ).toBe("/settings?section=music");
});
