import { createElement } from "react";
import { render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const { reload } = vi.hoisted(() => ({ reload: vi.fn() }));
vi.mock("@inertiajs/react", () => ({ router: { reload } }));

import { useAutomaticArtistMatching } from "../hooks/useAutomaticArtistMatching";

const account = "10000000-0000-4000-8000-000000000001";

function Probe() {
  useAutomaticArtistMatching(account);
  return null;
}

function receipt(
  body: { request_id: string; after?: string | null },
  matched: number,
  next: string | null,
) {
  return Response.json({
    request_id: body.request_id,
    matched,
    skipped_ambiguous: 0,
    skipped_declined: 0,
    next_after: next,
  });
}

function providerReceipt(
  body: { request_id: string },
  counts: { matched?: number; unavailable?: number },
  next: string | null,
) {
  return Response.json({
    request_id: body.request_id,
    matched: counts.matched ?? 0,
    uncertain: 0,
    declined: 0,
    unavailable: counts.unavailable ?? 0,
    next_after: next,
  });
}

beforeEach(() => {
  reload.mockReset();
  localStorage.clear();
});
afterEach(() => vi.unstubAllGlobals());

test("a pass walks every batch from the returned cursor and refreshes the page once when something matched", async () => {
  const afters: (string | null)[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      expect(options?.method).toBe("POST");
      const body = JSON.parse(options?.body as string);
      if (path === "/api/v1/artist-matching/provider-evidence")
        return providerReceipt(body, {}, null);
      expect(path).toBe("/api/v1/artist-matching");
      afters.push(body.after ?? null);
      if (afters.length === 1) return receipt(body, 2, "r-100");
      return receipt(body, 1, null);
    }),
  );
  render(createElement(Probe));
  await waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
  expect(afters).toEqual([null, "r-100"]);
  expect(reload).toHaveBeenCalledWith({ only: ["artists", "identity_groups"] });
});

test("an idle pass is silent and a recent pass is not repeated", async () => {
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const body = JSON.parse(options?.body as string);
    return path.endsWith("/provider-evidence")
      ? providerReceipt(body, {}, null)
      : receipt(body, 0, null);
  });
  vi.stubGlobal("fetch", fetch);
  const first = render(createElement(Probe));
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
  await waitFor(() =>
    expect(localStorage.getItem(`hito:artist-match:${account}`)).not.toBeNull(),
  );
  expect(reload).not.toHaveBeenCalled();
  first.unmount();
  render(createElement(Probe));
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(fetch).toHaveBeenCalledTimes(2);
});

test("a pass that is cancelled or fails still starts the pause, and nothing disturbs the page", async () => {
  const signals: AbortSignal[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((_path: string, options?: RequestInit) => {
      signals.push(options!.signal!);
      return new Promise<Response>((_resolve, reject) =>
        options!.signal!.addEventListener("abort", () =>
          reject(new DOMException("aborted", "AbortError")),
        ),
      );
    }),
  );
  const view = render(createElement(Probe));
  await waitFor(() => expect(signals).toHaveLength(1));
  view.unmount();
  expect(signals[0].aborted).toBe(true);
  expect(reload).not.toHaveBeenCalled();
  expect(
    localStorage.getItem(`hito:artist-match:${account}`),
    "The pause starts with the pass, so a cancelled or failing pass is not retried by every tab",
  ).not.toBeNull();
});

test("imported tracks group without a click: the provider pass follows the tag pass, walks its cursor and refreshes the page once", async () => {
  const calls: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      const body = JSON.parse(options?.body as string);
      calls.push(`${path}:${body.after ?? ""}`);
      if (path === "/api/v1/artist-matching") return receipt(body, 0, null);
      return body.after
        ? providerReceipt(body, { matched: 1 }, null)
        : providerReceipt(body, { matched: 2 }, "r-8");
    }),
  );
  render(createElement(Probe));
  await waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
  expect(calls).toEqual([
    "/api/v1/artist-matching:",
    "/api/v1/artist-matching/provider-evidence:",
    "/api/v1/artist-matching/provider-evidence:r-8",
  ]);
  expect(reload).toHaveBeenCalledWith({ only: ["artists", "identity_groups"] });
});

test("an unavailable provider ends the provider pass without retrying, and the page stays untouched", async () => {
  const providerCalls: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      const body = JSON.parse(options?.body as string);
      if (path === "/api/v1/artist-matching") return receipt(body, 0, null);
      providerCalls.push(body.after ?? "");
      return providerReceipt(body, { unavailable: 2 }, "r-8");
    }),
  );
  render(createElement(Probe));
  await waitFor(() => expect(providerCalls).toHaveLength(1));
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(providerCalls).toHaveLength(1);
  expect(reload).not.toHaveBeenCalled();
});
