import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import PodcastIndexPage from "./pages/PodcastIndexPage";
import { NoticeProvider } from "./components/Notices";
import {
  emptyPodcastPage,
  ownedPodcastFeed,
  ownedPodcastJob,
  readyPodcast,
} from "./fixtures/podcastSubscriptions";
import type { components } from "./generated";

vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

const responses = new Set<Response>();

afterEach(async () => {
  cleanup();
  await Promise.all(
    [...responses].map((response) =>
      !response.bodyUsed ? response.body?.cancel() : undefined,
    ),
  );
  responses.clear();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("Podcasts keeps an accepted existing subscription visible across stale same-account page props and permits another fresh check", async () => {
  const info = vi.spyOn(console, "info").mockImplementation(() => {});
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const requests: components["schemas"]["SubscribePodcast"][] = [];
  let initialReads = 0;
  let subscriptionReads = 0;
  function json(body: unknown, status = 200) {
    const response = Response.json(body, { status });
    responses.add(response);
    return response;
  }
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    expect(String(input)).toBe("/api/v1/podcasts");
    expect(options).toMatchObject({
      credentials: "same-origin",
      headers: {
        "x-hito-account": emptyPodcastPage.user.id,
        "x-hito-request": "1",
      },
      signal: expect.any(AbortSignal),
    });
    if (options?.method === "POST") {
      const request: components["schemas"]["SubscribePodcast"] = JSON.parse(
        String(options.body),
      );
      expect(request.feed_url).toBe(ownedPodcastFeed);
      requests.push(request);
      return json(
        {
          request_id: request.request_id,
          subscription_id: readyPodcast.subscription_id,
          job_id: ownedPodcastJob,
        } satisfies components["schemas"]["PodcastSubscriptionReceipt"],
        202,
      );
    }
    expect(options?.method).toBe("GET");
    if (requests.length === 0) {
      initialReads += 1;
      return json([]);
    }
    subscriptionReads += 1;
    return json([readyPodcast]);
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(createElement(PodcastIndexPage, emptyPodcastPage), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  await act(async () => {});
  expect(initialReads).toBe(1);
  expect(subscriptionReads).toBe(0);
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("link", { name: readyPodcast.title })).toBeNull();

  const address = screen.getByRole("textbox", { name: "Feed address" });
  fireEvent.change(address, { target: { value: ownedPodcastFeed } });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
  });
  expect(requests).toHaveLength(1);
  expect(subscriptionReads).toBe(1);
  expect(
    screen.getByRole("link", { name: readyPodcast.title }).getAttribute("href"),
  ).toBe(`/podcasts/${readyPodcast.subscription_id}`);
  expect(screen.queryByRole("alert")).toBeNull();

  view.rerender(
    createElement(PodcastIndexPage, { ...emptyPodcastPage, subscriptions: [] }),
  );
  expect
    .soft(
      screen.queryByRole("link", { name: readyPodcast.title }),
      "Stale page props must not silently erase the subscription confirmed by the HTTP list",
    )
    .not.toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();

  const readsBeforeCheck = subscriptionReads;
  fireEvent.change(address, { target: { value: ownedPodcastFeed } });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
  });
  expect(requests).toHaveLength(2);
  expect(requests[1].request_id).not.toBe(requests[0].request_id);
  expect(subscriptionReads).toBeGreaterThan(readsBeforeCheck);
  expect(screen.getByRole("link", { name: readyPodcast.title })).toBeTruthy();
  expect(screen.queryByRole("alert")).toBeNull();
  expect([...responses].every((response) => response.bodyUsed)).toBe(true);
  expect(initialReads).toBe(1);
  const logs = JSON.stringify([...info.mock.calls, ...warn.mock.calls]);
  expect(logs).not.toContain(ownedPodcastFeed);
  expect(logs).not.toContain(readyPodcast.title);
});
