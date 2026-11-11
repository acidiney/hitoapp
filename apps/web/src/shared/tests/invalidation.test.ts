import { afterEach, expect, test, vi } from "vitest";
import { changeMany, change } from "../sync";
import { invalidate, onInvalidated } from "../invalidation";

afterEach(() => vi.unstubAllGlobals());

function acknowledge() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_path: string, init: RequestInit) => {
      const batch = JSON.parse(String(init.body));
      return Response.json({
        acknowledged: batch.operations.map((o: { id: string }) => o.id),
      });
    }),
  );
}

test("an accepted like announces the recording to every view", async () => {
  acknowledge();
  const seen: (string | null)[] = [];
  const stop = onInvalidated("recording", (id) => seen.push(id));
  await change("alice", { kind: "like_set", recording_id: "r1", liked: true });
  stop();
  expect(seen).toEqual(["r1"]);
});

test("a rejected change announces nothing", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("no", { status: 500 })),
  );
  const seen: (string | null)[] = [];
  const stop = onInvalidated("recording", (id) => seen.push(id));
  await changeMany("alice", [
    { kind: "like_set", recording_id: "r1", liked: true },
  ]).catch(() => undefined);
  stop();
  expect(seen).toEqual([]);
});

test("playlist changes announce the playlists list and stop after unsubscribe", async () => {
  acknowledge();
  const seen: (string | null)[] = [];
  const stop = onInvalidated("playlists", (id) => seen.push(id));
  await change("alice", {
    kind: "playlist_rename",
    playlist_id: "p1",
    name: "x",
  } as never);
  stop();
  invalidate("playlists");
  expect(seen).toEqual([null]);
});
