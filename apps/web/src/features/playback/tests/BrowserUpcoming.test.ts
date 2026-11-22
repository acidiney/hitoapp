import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import { BrowserUpcoming, upcomingClientTag } from "../browser/BrowserUpcoming";

const PATH = "/api/v1/playback/upcoming";

type Sent = {
  body: Schema<"UpcomingRecordings">;
  headers: Headers;
  at: number;
};

let sent: Sent[];
let reply: () => Response | Promise<Response>;

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  sent = [];
  reply = () => new Response(null, { status: 204 });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      expect(String(input)).toBe(PATH);
      expect(options?.method).toBe("POST");
      sent.push({
        body: JSON.parse(String(options?.body)),
        headers: new Headers(options?.headers),
        at: Date.now(),
      });
      return reply();
    }),
  );
});

afterEach(() => vi.useRealTimers());

function announcer(account = "owned-account") {
  const controller = new AbortController();
  const upcoming = new BrowserUpcoming(account, () => ({
    signal: controller.signal,
    active: () => !controller.signal.aborted,
  }));
  return { upcoming, controller };
}

const wait = (milliseconds: number) =>
  vi.advanceTimersByTimeAsync(milliseconds);

test("a track start announces the current track and the next two after a short quiet period", async () => {
  const { upcoming } = announcer();
  upcoming.announce({ current: "a", next: ["b", "c"] });
  await wait(900);
  expect(sent).toHaveLength(0);
  await wait(200);
  expect(sent).toHaveLength(1);
  expect(sent[0].body).toEqual({
    client: upcomingClientTag(),
    current: "a",
    recording_ids: ["b", "c"],
  });
  expect(sent[0].headers.get("x-hito-request")).toBe("1");
  expect(sent[0].headers.get("x-hito-account")).toBe("owned-account");
});

test("quick changes collapse into the latest announcement and an unchanged one is not sent again", async () => {
  const { upcoming } = announcer();
  upcoming.announce({ current: "a", next: ["b", "c"] });
  await wait(300);
  upcoming.announce({ current: "a", next: ["c", "b"] });
  await wait(300);
  upcoming.announce({ current: "b", next: ["c"] });
  await wait(5000);
  expect(sent.map((item) => item.body)).toEqual([
    { client: upcomingClientTag(), current: "b", recording_ids: ["c"] },
  ]);
  upcoming.announce({ current: "b", next: ["c"] });
  await wait(5000);
  expect(sent).toHaveLength(1);
});

test("a queue change announces again with the new next tracks", async () => {
  const { upcoming } = announcer();
  upcoming.announce({ current: "a", next: ["b", "c"] });
  await wait(1100);
  upcoming.announce({ current: "a", next: ["d", "b"] });
  await wait(3000);
  expect(sent.map((item) => item.body.recording_ids)).toEqual([
    ["b", "c"],
    ["d", "b"],
  ]);
});

test("stopping announces that nothing is playing, and nothing is sent before anything played", async () => {
  const { upcoming } = announcer();
  upcoming.announce({ current: null, next: [] });
  await wait(5000);
  expect(sent).toHaveLength(0);
  upcoming.announce({ current: "a", next: ["b"] });
  await wait(1100);
  upcoming.announce({ current: null, next: [] });
  await wait(3000);
  expect(sent.map((item) => item.body)).toEqual([
    { client: upcomingClientTag(), current: "a", recording_ids: ["b"] },
    { client: upcomingClientTag(), current: null, recording_ids: [] },
  ]);
});

test("constant changes still announce every few seconds, never more than 30 a minute, and the last state arrives", async () => {
  const { upcoming } = announcer();
  for (let step = 0; step < 600; step++) {
    upcoming.announce({ current: `track-${step}`, next: [] });
    await wait(100);
  }
  expect(sent.length).toBeLessThanOrEqual(30);
  expect(sent.length).toBeGreaterThanOrEqual(10);
  await wait(3000);
  expect(sent.length).toBeLessThanOrEqual(31);
  expect(sent.at(-1)?.body.current).toBe("track-599");
  const gaps = sent.slice(1).map((item, index) => item.at - sent[index].at);
  expect(Math.min(...gaps)).toBeGreaterThanOrEqual(2000);
});

test("a 429 waits for retry-after and then sends only the latest state", async () => {
  const { upcoming } = announcer();
  reply = () =>
    new Response(JSON.stringify({ error: "Too many" }), {
      status: 429,
      headers: { "retry-after": "7", "content-type": "application/json" },
    });
  upcoming.announce({ current: "a", next: ["b"] });
  await wait(1100);
  expect(sent).toHaveLength(1);
  reply = () => new Response(null, { status: 204 });
  upcoming.announce({ current: "b", next: ["c"] });
  await wait(5000);
  expect(sent).toHaveLength(1);
  await wait(1000);
  expect(sent).toHaveLength(1);
  await wait(1100);
  expect(sent.map((item) => item.body.current)).toEqual(["a", "b"]);
  await wait(60_000);
  expect(sent).toHaveLength(2);
});

test("a 429 without retry-after waits a minute", async () => {
  const { upcoming } = announcer();
  reply = () => new Response(null, { status: 429 });
  upcoming.announce({ current: "a", next: [] });
  await wait(1100);
  reply = () => new Response(null, { status: 204 });
  await wait(58_000);
  expect(sent).toHaveLength(1);
  await wait(2000);
  expect(sent.map((item) => item.body.current)).toEqual(["a", "a"]);
});

test("failed announcements are dropped without retries and the next change is still sent", async () => {
  const { upcoming } = announcer();
  reply = () => {
    throw new TypeError("offline");
  };
  upcoming.announce({ current: "a", next: ["b"] });
  await wait(1100);
  reply = () => new Response(JSON.stringify({ error: "x" }), { status: 500 });
  upcoming.announce({ current: "b", next: [] });
  await wait(3000);
  expect(sent).toHaveLength(2);
  await wait(120_000);
  expect(sent).toHaveLength(2);
  reply = () => new Response(null, { status: 204 });
  upcoming.announce({ current: "c", next: [] });
  await wait(3000);
  expect(sent.map((item) => item.body.current)).toEqual(["a", "b", "c"]);
});

test("closing or leaving the account drops a pending announcement", async () => {
  const first = announcer();
  first.upcoming.announce({ current: "a", next: [] });
  first.upcoming.close();
  const second = announcer();
  second.upcoming.announce({ current: "b", next: [] });
  second.controller.abort();
  await wait(5000);
  expect(sent).toHaveLength(0);
});

test("one tab keeps one random tag that the server accepts", () => {
  const tag = upcomingClientTag();
  expect(tag).toMatch(/^web-[A-Za-z0-9]{12}$/);
  expect(upcomingClientTag()).toBe(tag);
  expect(sessionStorage.getItem("hito:playback-client")).toBe(tag);
});

test("the tag still works when session storage is unavailable", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new DOMException("blocked", "SecurityError");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new DOMException("blocked", "SecurityError");
  });
  const tag = upcomingClientTag();
  expect(tag).toMatch(/^web-[A-Za-z0-9]{12}$/);
  expect(upcomingClientTag()).toBe(tag);
});
