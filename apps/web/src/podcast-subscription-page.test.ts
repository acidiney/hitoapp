// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import PodcastIndexPage from "./pages/PodcastIndexPage";
import { NoticeProvider } from "./components/Notices";
import type { components } from "./generated";

vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("Podcasts recovers a lost subscription acknowledgement with the exact request and polls only while pending", async () => {
  const subscriptionId = "783e5d8c-5f51-453a-9d5e-627838cc02d5";
  const privateFeed = "https://feed.invalid/private-original-feed.xml";
  const otherFeed = "https://feed.invalid/private-edited-feed.xml";
  const privateTitle = "Owned private podcast";
  const info = vi.spyOn(console, "info").mockImplementation(() => {});
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const pending = {
    subscription_id: subscriptionId,
    title: null,
    refresh_pending: true,
    refresh_failed: false,
    last_refreshed_at: null,
    content_revision: 0,
  } satisfies components["schemas"]["PodcastSubscription"];
  const ready = {
    ...pending,
    title: privateTitle,
    refresh_pending: false,
    last_refreshed_at: 1_790_000_000,
    content_revision: 1,
  } satisfies components["schemas"]["PodcastSubscription"];
  const props = {
    user: { id: "alice", username: "Alice", admin: false },
    errors: {},
    native_playback: false,
    libraries: [],
    playlists: [],
    subscriptions: [],
  } satisfies components["schemas"]["PodcastIndexPageProps"];
  let initialReads = 0;
  let subscriptionsRead = 0;
  let acceptedRequest: string | null = null;
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    const path = String(input);
    if (path !== "/api/v1/podcasts")
      throw new Error("Unexpected protocol request");
    if (options?.method === "POST") {
      const body = String(options.body);
      if (acceptedRequest === null) {
        acceptedRequest = body;
        throw new TypeError("The accepted response was lost.");
      }
      expect(body).toBe(acceptedRequest);
      const request = JSON.parse(body);
      const receipt = {
        request_id: request.request_id,
        subscription_id: subscriptionId,
        job_id: "12b53675-cc86-4ffc-a94b-94897fbddcad",
      } satisfies components["schemas"]["PodcastSubscriptionReceipt"];
      return Response.json(receipt, { status: 202 });
    }
    if (acceptedRequest === null) {
      initialReads += 1;
      return Response.json([]);
    }
    subscriptionsRead += 1;
    return Response.json([subscriptionsRead === 1 ? pending : ready]);
  });
  vi.stubGlobal("fetch", fetch);
  render(createElement(PodcastIndexPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  await act(async () => {});
  expect(initialReads).toBe(1);
  expect(subscriptionsRead).toBe(0);
  expect(fetch.mock.calls[0][1]?.method).toBe("GET");

  const address = screen.getByRole("textbox", { name: "Feed address" });
  fireEvent.change(address, { target: { value: `  ${privateFeed}  ` } });
  expect(fetch).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
  await screen.findByRole("alert");
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
  expect(fetch.mock.calls[1][1]).toMatchObject({
    method: "POST",
    credentials: "same-origin",
    headers: { "x-hito-account": "alice", "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  expect(JSON.parse(acceptedRequest!)).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
    feed_url: privateFeed,
  });
  fireEvent.change(address, { target: { value: otherFeed } });
  expect(
    (screen.getByRole("button", { name: "Subscribe" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);

  vi.useFakeTimers();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Retry subscription" }));
  });
  expect(fetch).toHaveBeenCalledTimes(4);
  expect(String(fetch.mock.calls[2][1]?.body)).toBe(acceptedRequest);
  expect(screen.getByText("Checking feed…")).toBeTruthy();
  expect(
    screen.getByRole("link", { name: "Untitled podcast" }).getAttribute("href"),
  ).toBe(`/podcasts/${subscriptionId}`);

  await act(async () => {
    await vi.advanceTimersByTimeAsync(5_000);
  });
  expect(subscriptionsRead).toBe(2);
  expect(screen.queryByText("Checking feed…")).toBeNull();
  expect(screen.getByRole("link", { name: privateTitle })).toBeTruthy();
  expect(screen.getByText(/Last checked/)).toBeTruthy();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(subscriptionsRead).toBe(2);
  expect(initialReads).toBe(1);
  expect(fetch).toHaveBeenCalledTimes(5);
  const logs = JSON.stringify([...info.mock.calls, ...warn.mock.calls]);
  expect(logs).not.toContain(privateFeed);
  expect(logs).not.toContain(otherFeed);
  expect(logs).not.toContain(privateTitle);
});

test("Podcasts permits a corrected feed with a new request after a definitive server rejection", async () => {
  const rejectedFeed = "https://feed.invalid/rejected-feed.xml";
  const correctedFeed = "https://feed.invalid/corrected-feed.xml";
  const subscriptionId = "dd416683-90da-41a9-aa2b-a6299c7e099a";
  const props = {
    user: { id: "alice", username: "Alice", admin: false },
    errors: {},
    native_playback: false,
    libraries: [],
    playlists: [],
    subscriptions: [],
  } satisfies components["schemas"]["PodcastIndexPageProps"];
  const requests: components["schemas"]["SubscribePodcast"][] = [];
  let initialReads = 0;
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    expect(String(input)).toBe("/api/v1/podcasts");
    if (options?.method === "POST") {
      const body = JSON.parse(String(options.body));
      requests.push(body);
      if (requests.length === 1)
        return Response.json(
          { error: "This feed address is not allowed." },
          { status: 400 },
        );
      const receipt = {
        request_id: body.request_id,
        subscription_id: subscriptionId,
        job_id: "ca72b4f4-e956-42ee-aeab-1ef00b5ba011",
      } satisfies components["schemas"]["PodcastSubscriptionReceipt"];
      return Response.json(receipt, { status: 202 });
    }
    if (requests.length < 2) {
      initialReads += 1;
      return Response.json([]);
    }
    return Response.json([
      {
        subscription_id: subscriptionId,
        title: "Corrected podcast",
        refresh_pending: false,
        refresh_failed: false,
        last_refreshed_at: 1_790_000_000,
        content_revision: 1,
      } satisfies components["schemas"]["PodcastSubscription"],
    ]);
  });
  vi.stubGlobal("fetch", fetch);
  render(createElement(PodcastIndexPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  await act(async () => {});
  expect(initialReads).toBe(1);
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch.mock.calls[0][1]?.method).toBe("GET");
  expect(screen.queryByRole("link", { name: "Corrected podcast" })).toBeNull();

  const address = screen.getByRole("textbox", { name: "Feed address" });
  fireEvent.change(address, { target: { value: rejectedFeed } });
  fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
  await screen.findByRole("alert");
  expect(requests).toHaveLength(1);
  expect(requests[0].feed_url).toBe(rejectedFeed);
  expect(
    screen.queryByRole("button", { name: "Retry subscription" }),
  ).toBeNull();
  fireEvent.change(address, { target: { value: correctedFeed } });
  expect(
    (screen.getByRole("button", { name: "Subscribe" }) as HTMLButtonElement)
      .disabled,
  ).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
  await screen.findByRole("link", { name: "Corrected podcast" });
  expect(requests).toHaveLength(2);
  expect(requests[1].feed_url).toBe(correctedFeed);
  expect(requests[1].request_id).not.toBe(requests[0].request_id);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(initialReads).toBe(1);
  expect(fetch).toHaveBeenCalledTimes(4);
});

test("Podcasts preserves a new feed draft while the submitted feed awaits acknowledgement", async () => {
  const submittedFeed = "https://feed.invalid/submitted-feed.xml";
  const nextFeed = "https://feed.invalid/new-draft.xml";
  const subscriptionId = "79565411-a9f4-4145-9b87-d78df2358697";
  const props = {
    user: { id: "alice", username: "Alice", admin: false },
    errors: {},
    native_playback: false,
    libraries: [],
    playlists: [],
    subscriptions: [],
  } satisfies components["schemas"]["PodcastIndexPageProps"];
  let resolveSubscription!: (response: Response) => void;
  const subscriptionResponse = new Promise<Response>((resolve) => {
    resolveSubscription = resolve;
  });
  let acknowledged = false;
  let initialReads = 0;
  let subscriptionsRead = 0;
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, options) => {
    expect(String(input)).toBe("/api/v1/podcasts");
    if (options?.method === "POST") return subscriptionResponse;
    if (!acknowledged) {
      initialReads += 1;
      return Response.json([]);
    }
    subscriptionsRead += 1;
    return Response.json([
      {
        subscription_id: subscriptionId,
        title: "Submitted podcast",
        refresh_pending: false,
        refresh_failed: false,
        last_refreshed_at: 1_790_000_000,
        content_revision: 1,
      } satisfies components["schemas"]["PodcastSubscription"],
    ]);
  });
  vi.stubGlobal("fetch", fetch);
  render(createElement(PodcastIndexPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  await act(async () => {});
  expect(initialReads).toBe(1);
  expect(subscriptionsRead).toBe(0);
  expect(fetch.mock.calls[0][1]?.method).toBe("GET");
  const address = screen.getByRole("textbox", {
    name: "Feed address",
  }) as HTMLInputElement;
  fireEvent.change(address, { target: { value: submittedFeed } });
  fireEvent.click(screen.getByRole("button", { name: "Subscribe" }));
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
  const originalBody = String(fetch.mock.calls[1][1]?.body);
  const request = JSON.parse(originalBody);
  expect(request.feed_url).toBe(submittedFeed);
  fireEvent.change(address, { target: { value: nextFeed } });
  expect(address.value).toBe(nextFeed);
  expect(fetch).toHaveBeenCalledTimes(2);
  const receipt = {
    request_id: request.request_id,
    subscription_id: subscriptionId,
    job_id: "87812118-6f12-4169-adc9-17c2cb0724c3",
  } satisfies components["schemas"]["PodcastSubscriptionReceipt"];
  await act(async () => {
    acknowledged = true;
    resolveSubscription(Response.json(receipt, { status: 202 }));
  });
  await screen.findByRole("link", { name: "Submitted podcast" });
  expect(screen.getByRole("textbox", { name: "Feed address" })).toBe(address);
  expect(address.value).toBe(nextFeed);
  expect(
    (screen.getByRole("button", { name: "Subscribe" }) as HTMLButtonElement)
      .disabled,
  ).toBe(false);
  expect(String(fetch.mock.calls[1][1]?.body)).toBe(originalBody);
  expect(initialReads).toBe(1);
  expect(subscriptionsRead).toBe(1);
  expect(fetch).toHaveBeenCalledTimes(3);
});
