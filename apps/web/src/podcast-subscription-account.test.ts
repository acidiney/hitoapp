import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import PodcastIndexPage from "./pages/PodcastIndexPage";
import { NoticeProvider } from "./shared/ui/Notices";
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

test("Podcasts discards a late previous-account list and recovers through the new account's own subscription", async () => {
  const info = vi.spyOn(console, "info").mockImplementation(() => {});
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const bobPodcast = {
    ...readyPodcast,
    subscription_id: "c8c5fa82-25b7-4a7e-a427-69226b149ac7",
    title: "Bob owned private podcast",
  } satisfies components["schemas"]["PodcastSubscription"];
  const bobPage = {
    ...emptyPodcastPage,
    user: { id: "bob", username: "Bob", admin: false },
    subscriptions: [],
  } satisfies components["schemas"]["PodcastIndexPageProps"];
  const bobFeed = "https://feed.invalid/bob-owned-private-podcast.xml";
  const bobJob = "8ebd9ebd-5b34-439f-aa12-72f64a7811b9";
  const encoder = new TextEncoder();
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  let bodyFinished = false;
  const lateResponse = new Response(
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
  const responses = [lateResponse];
  const reads: { account: string; signal: AbortSignal }[] = [];
  const writes: { account: string; feed: string }[] = [];
  function json(body: unknown, status = 200) {
    const response = Response.json(body, { status });
    responses.push(response);
    return response;
  }
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    expect(String(input)).toBe("/api/v1/podcasts");
    expect(options?.credentials).toBe("same-origin");
    const headers = new Headers(options?.headers);
    expect(headers.get("x-hito-request")).toBe("1");
    const account = headers.get("x-hito-account");
    if (account !== emptyPodcastPage.user.id && account !== bobPage.user.id)
      throw new Error("Unexpected podcast account");
    if (!(options?.signal instanceof AbortSignal))
      throw new Error("Podcast requests require an owned abort signal");
    expect(options.signal.aborted).toBe(false);
    const bob = account === bobPage.user.id;
    if (options.method === "POST") {
      const request: components["schemas"]["SubscribePodcast"] = JSON.parse(
        String(options.body),
      );
      expect(request.feed_url).toBe(bob ? bobFeed : ownedPodcastFeed);
      writes.push({ account, feed: request.feed_url });
      return json(
        {
          request_id: request.request_id,
          subscription_id: bob
            ? bobPodcast.subscription_id
            : readyPodcast.subscription_id,
          job_id: bob ? bobJob : ownedPodcastJob,
        } satisfies components["schemas"]["PodcastSubscriptionReceipt"],
        202,
      );
    }
    expect(options.method).toBe("GET");
    reads.push({ account, signal: options.signal });
    if (!writes.some((write) => write.account === account)) return json([]);
    if (bob) return json([bobPodcast]);
    const accountReads = reads.filter(
      (read) => read.account === account,
    ).length;
    if (accountReads === 2) return json([readyPodcast]);
    expect(accountReads).toBe(3);
    return lateResponse;
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(createElement(PodcastIndexPage, emptyPodcastPage), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  try {
    await act(async () => {});
    expect(reads).toHaveLength(1);
    expect(reads[0].account).toBe(emptyPodcastPage.user.id);
    expect(writes).toHaveLength(0);
    expect(screen.queryByRole("link", { name: readyPodcast.title })).toBeNull();
    fireEvent.change(screen.getByRole("textbox", { name: "Feed address" }), {
      target: { value: ownedPodcastFeed },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
    });
    expect(reads).toHaveLength(2);
    expect(screen.getByRole("link", { name: readyPodcast.title })).toBeTruthy();
    await act(async () => {
      view.rerender(
        createElement(PodcastIndexPage, {
          ...emptyPodcastPage,
          subscriptions: [],
        }),
      );
    });
    expect(reads).toHaveLength(3);
    const heldRead = reads[2];
    expect(heldRead.account).toBe(emptyPodcastPage.user.id);
    expect(heldRead.signal.aborted).toBe(false);
    expect(lateResponse.bodyUsed).toBe(true);
    expect(bodyFinished).toBe(false);

    view.rerender(createElement(PodcastIndexPage, bobPage));
    expect(screen.queryByRole("link", { name: readyPodcast.title })).toBeNull();
    expect(heldRead.signal.aborted).toBe(true);
    expect(screen.queryByRole("alert")).toBeNull();
    await act(async () => {
      bodyFinished = true;
      controller.enqueue(encoder.encode(`${JSON.stringify(readyPodcast)}]`));
      controller.close();
    });
    expect(screen.queryByRole("link", { name: readyPodcast.title })).toBeNull();
    expect(screen.queryByRole("link", { name: bobPodcast.title })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();

    const bobReadsBeforeSubmit = reads.filter(
      (read) => read.account === bobPage.user.id,
    ).length;
    expect(bobReadsBeforeSubmit).toBe(1);
    fireEvent.change(screen.getByRole("textbox", { name: "Feed address" }), {
      target: { value: bobFeed },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
    });
    expect(writes).toEqual([
      { account: emptyPodcastPage.user.id, feed: ownedPodcastFeed },
      { account: bobPage.user.id, feed: bobFeed },
    ]);
    expect(
      reads.filter((read) => read.account === bobPage.user.id).length,
    ).toBeGreaterThan(bobReadsBeforeSubmit);
    expect(reads).toHaveLength(5);
    expect(
      screen.getByRole("link", { name: bobPodcast.title }).getAttribute("href"),
    ).toBe(`/podcasts/${bobPodcast.subscription_id}`);
    expect(screen.queryByRole("link", { name: readyPodcast.title })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(responses.every((response) => response.bodyUsed)).toBe(true);
    const logs = JSON.stringify([...info.mock.calls, ...warn.mock.calls]);
    expect(logs).not.toContain(ownedPodcastFeed);
    expect(logs).not.toContain(readyPodcast.title);
    expect(logs).not.toContain(bobFeed);
    expect(logs).not.toContain(bobPodcast.title);
  } finally {
    view.unmount();
    await act(async () => {
      if (!bodyFinished) {
        if (lateResponse.bodyUsed) {
          bodyFinished = true;
          controller.error(
            new DOMException("Owned response disposed", "AbortError"),
          );
        } else {
          await lateResponse.body?.cancel();
        }
      }
    });
    await Promise.all(
      responses.map((response) =>
        !response.bodyUsed ? response.body?.cancel() : undefined,
      ),
    );
  }
  expect(reads.every((read) => read.signal.aborted)).toBe(true);
});
