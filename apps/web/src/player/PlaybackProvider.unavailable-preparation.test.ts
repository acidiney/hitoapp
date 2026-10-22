import { act, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { ResolveSource, SyncBatch } from "../shared/types";
import {
  audioPath,
  browserPreparationMedia,
  deferred,
} from "./fixtures/browserPreparationMedia";
import {
  cachedSourcesPath,
  ownedPlaybackHttp,
  ownedSource,
  ownedTrack,
  renderPlaybackAccount,
  sourcesPath,
  unavailableResponse,
  type OwnedPlaybackRequest,
} from "./fixtures/browserSourceResolution";

const first = ownedTrack("f88f701f-a352-41a0-abda-21fc76b8a387");
const next = ownedTrack("74aa28ac-1583-4df0-8252-484e6c5ce069");
const after = ownedTrack("68d155c9-2894-4980-b22b-8855dafb6095");
const firstSource = ownedSource(first, "34aa1ecf-d01b-4b1f-9dab-2f3e40aff28b");
const nextSource = ownedSource(next, "563864ed-08b8-41eb-94c3-2948e17d0b0e");
const afterSource = ownedSource(after, "2b77f558-ac64-4431-a097-9d141f80f0f9");
const unavailableMessage =
  "An audio service is unavailable. Select the track again to retry.";

test.each([false, true])(
  "a prepared service outage preserves current audio and rechecks ready alternatives at the boundary, ready=%s",
  async (newReady) => {
    const media = browserPreparationMedia();
    const entered = deferred<OwnedPlaybackRequest>();
    const acknowledgement = deferred<Response>();
    let boundary = false;
    const { requests, unexpected } = ownedPlaybackHttp((request) => {
      if (request.path === cachedSourcesPath(first) && request.method === "GET")
        return Response.json([firstSource]);
      if (request.path === cachedSourcesPath(next) && request.method === "GET")
        return Response.json(boundary && newReady ? [nextSource] : []);
      if (request.path === cachedSourcesPath(after) && request.method === "GET")
        return Response.json([afterSource]);
      if (
        request.path === `${sourcesPath(next)}/refresh` &&
        request.method === "POST"
      ) {
        expect(request.body).toEqual({ purpose: "playback" });
        return Response.json([]);
      }
      if (
        request.path === `${sourcesPath(next)}/resolve` &&
        request.method === "POST"
      ) {
        expect(Object.keys(request.body as ResolveSource)).toEqual([
          "request_id",
        ]);
        entered.resolve(request);
        return acknowledgement.promise;
      }
      if (request.path === "/api/v1/sync" && request.method === "POST") {
        const batch = request.body as SyncBatch;
        return Response.json({
          acknowledged: batch.operations.map((operation) => operation.id),
        });
      }
      throw new Error("Unexpected owned playback workflow request");
    });
    const { players, unmount } = renderPlaybackAccount("alice");
    const player = players.get("alice")!;
    try {
      await act(async () => {
        await player.playQueue([first, next, after]);
      });
      const pending = await entered.promise;
      const entries = (await player.queuePage(0, 10)).entries;
      const current = media.created[0];
      const loaded = [...media.loaded];
      expect(media.played.map((play) => play.source)).toEqual([
        audioPath(firstSource.id),
      ]);
      await act(async () => {
        acknowledgement.resolve(unavailableResponse(pending, next));
        await acknowledgement.promise;
      });
      act(() => {
        current.currentTime = 5;
        current.dispatchEvent(new Event("timeupdate"));
      });
      expect(player.snapshot().current?.id).toBe(first.id);
      expect(player.snapshot().playing).toBe(true);
      expect(player.snapshot().error).toBe("");
      expect(player.positionSnapshot().position).toBe(5);
      expect(current.paused).toBe(false);
      expect(media.loaded).toEqual(loaded);
      expect(media.created).toHaveLength(1);
      expect(
        requests.filter((request) => request.path.endsWith("/resolve")),
      ).toHaveLength(1);

      boundary = true;
      act(() => {
        media.end(current);
      });
      await vi.waitFor(() => {
        expect(player.snapshot().current?.id).toBe(
          newReady ? next.id : after.id,
        );
        expect(player.snapshot().playing).toBe(true);
      });
      expect(player.snapshot().error).toBe(newReady ? "" : unavailableMessage);
      if (newReady) expect(screen.queryByRole("status")).toBeNull();
      else
        expect(screen.getByRole("status").textContent).toBe(unavailableMessage);
      expect(media.played.map((play) => play.source)).toEqual([
        audioPath(firstSource.id),
        audioPath(newReady ? nextSource.id : afterSource.id),
      ]);
      expect((await player.queuePage(0, 10)).entries).toEqual(entries);
      expect(player.queueSnapshot().currentEntry).toBe(
        entries[newReady ? 1 : 2].id,
      );
      expect(
        requests.filter(
          (request) => request.path === `${sourcesPath(next)}/resolve`,
        ),
      ).toEqual([pending]);
      expect(
        requests.filter((request) => request.path === cachedSourcesPath(next)),
      ).toHaveLength(2);
      expect(
        requests
          .filter((request) => request.path === "/api/v1/sync")
          .flatMap((request) =>
            (request.body as SyncBatch).operations.map(
              (operation) => operation.action,
            ),
          ),
      ).toEqual([
        {
          kind: "progress_set",
          recording_id: first.id,
          position_ms: 90000,
          speed: 1,
        },
      ]);
      expect(requests.every((request) => request.account === "alice")).toBe(
        true,
      );
      expect(unexpected).toEqual([]);
    } finally {
      acknowledgement.resolve(Response.json(null));
      unmount();
      media.close();
    }
    expect(document.querySelectorAll("audio")).toHaveLength(0);
  },
);

test("a late accepted outage from a retired account cannot alter the new account player", async () => {
  const media = browserPreparationMedia();
  const entered = deferred<OwnedPlaybackRequest>();
  const acknowledgement = deferred<Response>();
  const { requests, unexpected } = ownedPlaybackHttp((request) => {
    if (request.account === "alice") {
      if (request.path === cachedSourcesPath(first) && request.method === "GET")
        return Response.json([firstSource]);
      if (request.path === cachedSourcesPath(next) && request.method === "GET")
        return Response.json([]);
      if (
        request.path === `${sourcesPath(next)}/refresh` &&
        request.method === "POST"
      )
        return Response.json([]);
      if (
        request.path === `${sourcesPath(next)}/resolve` &&
        request.method === "POST"
      ) {
        entered.resolve(request);
        return acknowledgement.promise;
      }
    }
    if (
      request.account === "bob" &&
      request.path === cachedSourcesPath(after) &&
      request.method === "GET"
    )
      return Response.json([afterSource]);
    throw new Error("Unexpected owned account request");
  });
  const { players, view, rerender, unmount } = renderPlaybackAccount("alice");
  try {
    const alice = players.get("alice")!;
    await act(async () => {
      await alice.playQueue([first, next]);
    });
    const pending = await entered.promise;
    expect(pending.signal.aborted).toBe(false);
    rerender(view("bob"));
    expect(pending.signal.aborted).toBe(true);
    expect(alice.snapshot().current).toBeNull();
    expect((await alice.queuePage(0, 10)).entries).toEqual([]);
    const bob = players.get("bob")!;
    await act(async () => {
      await bob.playQueue([after]);
    });
    const before = bob.snapshot();
    const entries = (await bob.queuePage(0, 10)).entries;
    const published = vi.fn();
    const stop = bob.subscribe(published);
    const count = requests.length;
    await act(async () => {
      acknowledgement.resolve(unavailableResponse(pending, next));
      await acknowledgement.promise;
    });
    stop();
    expect(published).not.toHaveBeenCalled();
    expect(bob.snapshot()).toBe(before);
    expect(bob.snapshot().playing).toBe(true);
    expect(bob.snapshot().error).toBe("");
    expect((await bob.queuePage(0, 10)).entries).toEqual(entries);
    expect(requests).toHaveLength(count);
    expect(screen.queryByRole("status")).toBeNull();
    expect(media.played.map((play) => play.source)).toEqual([
      audioPath(firstSource.id),
      audioPath(afterSource.id),
    ]);
    expect(document.querySelectorAll("audio")).toHaveLength(1);
    expect(unexpected).toEqual([]);
  } finally {
    acknowledgement.resolve(Response.json(null));
    unmount();
    media.close();
  }
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
