import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { reload } = vi.hoisted(() => ({ reload: vi.fn() }));
vi.mock("@inertiajs/react", () => ({ router: { reload } }));

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

const button = () =>
  screen.getByRole("button", { name: "Find matches on MusicBrainz" });

beforeEach(() => reload.mockReset());
afterEach(() => vi.unstubAllGlobals());

test("the page explains what is sent and sends nothing until the button is clicked", async () => {
  const calls = stub(
    () => settings(true),
    (body) => receipt(body, { matched: 1 }, null),
  );
  render(createElement(ProviderArtistMatching, { account }));
  expect(
    screen.getByText(
      "Sends ISRC codes and provider artist links from your library to MusicBrainz.",
    ),
  ).toBeTruthy();
  await waitFor(() =>
    expect((button() as HTMLButtonElement).disabled).toBe(false),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(calls).toEqual([SETTINGS]);
});

test("a click walks the cursor, shows the receipt counts and refreshes the artists once", async () => {
  const afters: (string | undefined)[] = [];
  stub(
    () => settings(true),
    (body) => {
      afters.push(body.after);
      return body.after
        ? receipt(body, { matched: 1, unavailable: 0, uncertain: 2 }, null)
        : receipt(body, { matched: 2, uncertain: 1 }, "r-8");
    },
  );
  render(createElement(ProviderArtistMatching, { account }));
  await waitFor(() =>
    expect((button() as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.click(button());
  await screen.findByText("3 matched, 3 uncertain, 0 unavailable.");
  expect(afters).toEqual([undefined, "r-8"]);
  expect(reload).toHaveBeenCalledTimes(1);
  expect(reload).toHaveBeenCalledWith({ only: ["artists", "identity_groups"] });
});

test("an unavailable provider ends the walk without retrying and is reported", async () => {
  const run = vi.fn((body: { request_id: string }) =>
    receipt(body, { unavailable: 2 }, "r-8"),
  );
  stub(() => settings(true), run);
  render(createElement(ProviderArtistMatching, { account }));
  await waitFor(() =>
    expect((button() as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.click(button());
  await screen.findByText(
    "0 matched, 0 uncertain, 2 unavailable. Run again to continue.",
  );
  expect(run).toHaveBeenCalledTimes(1);
  expect(reload).not.toHaveBeenCalled();
});

test("the button stays disabled and nothing is sent when metadata enrichment is off or the built-in provider is not selected", async () => {
  for (const policy of [() => settings(false), () => settings(true, false)]) {
    const calls = stub(policy, (body) => receipt(body, {}, null));
    const view = render(createElement(ProviderArtistMatching, { account }));
    await screen.findByText(/Turn on the built-in MusicBrainz provider/);
    expect((button() as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(button());
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(calls).toEqual([SETTINGS]);
    view.unmount();
  }
});

test("leaving the page cancels a running search", async () => {
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
  await waitFor(() =>
    expect((button() as HTMLButtonElement).disabled).toBe(false),
  );
  fireEvent.click(button());
  await waitFor(() => expect(signals).toHaveLength(1));
  view.unmount();
  expect(signals[0].aborted).toBe(true);
  expect(reload).not.toHaveBeenCalled();
});
