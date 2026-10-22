import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import PodcastIndexPage from "../pages/PodcastIndexPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import {
  emptyPodcastPage,
  ownedPodcastFeed,
  ownedPodcastJob,
  readyPodcast,
} from "../fixtures/podcastSubscriptions";
import type { components } from "../../../generated";

vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

test("Podcasts keeps its checked card during page validation and accepts a fresh empty HTTP list", async () => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  const encoder = new TextEncoder();
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  let bodyFinished = false;
  const response = new Response(
    new ReadableStream<Uint8Array>({
      start(stream) {
        controller = stream;
        stream.enqueue(encoder.encode("["));
      },
      cancel() {
        bodyFinished = true;
      },
    }),
    { headers: { "content-type": "application/json" } },
  );
  const responses = [response];
  const readSignals: AbortSignal[] = [];
  let initialReads = 0;
  let posts = 0;
  function json(body: unknown, status = 200) {
    const complete = Response.json(body, { status });
    responses.push(complete);
    return complete;
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
      posts += 1;
      const request: components["schemas"]["SubscribePodcast"] = JSON.parse(
        String(options.body),
      );
      expect(request.feed_url).toBe(ownedPodcastFeed);
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
    readSignals.push(options.signal);
    if (posts === 0) {
      initialReads += 1;
      return json([]);
    }
    if (readSignals.length === 2) return json([readyPodcast]);
    expect(readSignals).toHaveLength(3);
    return response;
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(createElement(PodcastIndexPage, emptyPodcastPage), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  try {
    await act(async () => {});
    expect(initialReads).toBe(1);
    expect(readSignals).toHaveLength(1);
    expect(screen.queryByRole("link", { name: readyPodcast.title })).toBeNull();
    fireEvent.change(screen.getByRole("textbox", { name: "Feed address" }), {
      target: { value: ownedPodcastFeed },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
    });
    expect(posts).toBe(1);
    expect(readSignals).toHaveLength(2);
    expect(screen.getByRole("link", { name: readyPodcast.title })).toBeTruthy();

    await act(async () => {
      view.rerender(
        createElement(PodcastIndexPage, {
          ...emptyPodcastPage,
          subscriptions: [],
        }),
      );
    });
    expect(readSignals).toHaveLength(3);
    expect(readSignals[2].aborted).toBe(false);
    expect(response.bodyUsed).toBe(true);
    expect(bodyFinished).toBe(false);
    expect(screen.getByRole("link", { name: readyPodcast.title })).toBeTruthy();
    expect(
      screen.queryByRole("heading", { name: "No podcast subscriptions yet" }),
    ).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();

    await act(async () => {
      bodyFinished = true;
      controller.enqueue(encoder.encode("]"));
      controller.close();
    });
    expect(screen.queryByRole("link", { name: readyPodcast.title })).toBeNull();
    expect(
      screen.getByRole("heading", { name: "No podcast subscriptions yet" }),
    ).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(initialReads).toBe(1);
    expect(readSignals).toHaveLength(3);
    expect(responses.every((item) => item.bodyUsed)).toBe(true);
  } finally {
    view.unmount();
    await act(async () => {
      if (!bodyFinished) {
        bodyFinished = true;
        controller.error(
          new DOMException("Owned response disposed", "AbortError"),
        );
      }
    });
    await Promise.all(
      responses.map((item) =>
        !item.bodyUsed ? item.body?.cancel() : undefined,
      ),
    );
  }
  expect(readSignals.every((signal) => signal.aborted)).toBe(true);
});
