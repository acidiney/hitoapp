// @vitest-environment jsdom
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

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("Podcasts reloads current subscriptions after returning with the original empty page props", async () => {
  const info = vi.spyOn(console, "info").mockImplementation(() => {});
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const responses: Response[] = [];
  const reads: AbortSignal[] = [];
  const writes: components["schemas"]["SubscribePodcast"][] = [];
  function json(body: unknown, status = 200) {
    const response = Response.json(body, { status });
    responses.push(response);
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
    });
    if (!(options?.signal instanceof AbortSignal))
      throw new Error("Podcast requests require an owned abort signal");
    expect(options.signal.aborted).toBe(false);
    if (options.method === "POST") {
      const request: components["schemas"]["SubscribePodcast"] = JSON.parse(
        String(options.body),
      );
      expect(request.feed_url).toBe(ownedPodcastFeed);
      writes.push(request);
      return json(
        {
          request_id: request.request_id,
          subscription_id: readyPodcast.subscription_id,
          job_id: ownedPodcastJob,
        } satisfies components["schemas"]["PodcastSubscriptionReceipt"],
        202,
      );
    }
    expect(options.method).toBe("GET");
    reads.push(options.signal);
    return json(writes.length > 0 ? [readyPodcast] : []);
  });
  vi.stubGlobal("fetch", fetch);
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(NoticeProvider, null, children);
  const firstVisit = render(createElement(PodcastIndexPage, emptyPodcastPage), {
    wrapper,
  });
  try {
    await act(async () => {});
    fireEvent.change(screen.getByRole("textbox", { name: "Feed address" }), {
      target: { value: ownedPodcastFeed },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
    });
    expect(writes).toHaveLength(1);
    expect(screen.getByRole("link", { name: readyPodcast.title })).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();

    firstVisit.unmount();
    expect(reads.every((signal) => signal.aborted)).toBe(true);
    const readsBeforeReturn = reads.length;
    const returnVisit = render(
      createElement(PodcastIndexPage, emptyPodcastPage),
      { wrapper },
    );
    try {
      await act(async () => {});
      expect(
        reads.length,
        "Returning with stale history props must read the current subscription list",
      ).toBeGreaterThan(readsBeforeReturn);
      expect(writes).toHaveLength(1);
      expect(
        screen
          .getByRole("link", { name: readyPodcast.title })
          .getAttribute("href"),
      ).toBe(`/podcasts/${readyPodcast.subscription_id}`);
      expect(
        screen.queryByRole("heading", { name: "No podcast subscriptions yet" }),
      ).toBeNull();
      expect(screen.queryByRole("alert")).toBeNull();
      expect(responses.every((response) => response.bodyUsed)).toBe(true);
      const logs = JSON.stringify([...info.mock.calls, ...warn.mock.calls]);
      expect(logs).not.toContain(ownedPodcastFeed);
      expect(logs).not.toContain(readyPodcast.title);
    } finally {
      returnVisit.unmount();
    }
  } finally {
    firstVisit.unmount();
    await act(async () => {});
    await Promise.all(
      responses.map((response) =>
        !response.bodyUsed ? response.body?.cancel() : undefined,
      ),
    );
  }
  expect(reads.every((signal) => signal.aborted)).toBe(true);
});
