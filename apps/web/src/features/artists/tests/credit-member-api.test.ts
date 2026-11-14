import { afterEach, expect, test, vi } from "vitest";
import { creditMemberApi } from "../api/creditMemberApi";

afterEach(() => vi.unstubAllGlobals());

test("member review and identity calls go to that member's own routes", async () => {
  const calls: { url: string; method: string; body: unknown }[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({
        url,
        method: init?.method ?? "GET",
        body: init?.body ? JSON.parse(String(init.body)) : null,
      });
      return Response.json({});
    }),
  );
  const signal = new AbortController().signal;
  const context = { artist_id: "guest" } as never;
  await creditMemberApi.context(
    "account",
    "rec/1",
    "guest id",
    { provider: { kind: "musicbrainz" } } as never,
    signal,
  );
  await creditMemberApi.search(
    "account",
    "rec/1",
    "guest id",
    { context, search: { query: "Guest", limit: 10, cursor: null } } as never,
    signal,
  );
  await creditMemberApi.preview(
    "account",
    "rec/1",
    "guest id",
    { context, provider_artist_id: "mbid" },
    signal,
  );
  await creditMemberApi.confirm(
    "account",
    "rec/1",
    "guest id",
    { context, provider_artist_id: "mbid" },
    signal,
  );
  await creditMemberApi.choice("account", "rec/1", "guest id", signal);
  await creditMemberApi.choose(
    "account",
    "rec/1",
    "guest id",
    {
      request_id: "00000000-0000-4000-8000-000000000001",
      expected_revision: 0,
      association_id: "00000000-0000-4000-8000-000000000002",
      context,
    },
    signal,
  );
  const member = "/api/v1/recordings/rec%2F1/credit-members/guest%20id";
  expect(calls.map(({ url, method }) => `${method} ${url}`)).toEqual([
    `POST ${member}/artist-review/context`,
    `POST ${member}/artist-review/search`,
    `POST ${member}/artist-review/preview`,
    `POST ${member}/artist-review/confirm`,
    `GET ${member}/artist-identity`,
    `PUT ${member}/artist-identity`,
  ]);
  expect(calls[3].body).toEqual({ context, provider_artist_id: "mbid" });
  expect(calls[5].body).toMatchObject({ expected_revision: 0, context });
});
