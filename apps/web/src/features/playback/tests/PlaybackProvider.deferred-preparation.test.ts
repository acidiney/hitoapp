import type { Schema } from "../../../shared/schema";
import { act, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import {
  audioPath,
  browserPreparationMedia,
  deferred,
} from "../fixtures/browserPreparationMedia";
import {
  cachedSourcesPath,
  ownedPlaybackHttp,
  ownedSource,
  ownedTrack,
  renderPlaybackAccount,
  sourcesPath,
  type OwnedPlaybackRequest,
} from "../fixtures/browserSourceResolution";

const first = ownedTrack("f88f701f-a352-41a0-abda-21fc76b8a387");
const next = ownedTrack("74aa28ac-1583-4df0-8252-484e6c5ce069");
const after = ownedTrack("68d155c9-2894-4980-b22b-8855dafb6095");
const firstSource = ownedSource(first, "34aa1ecf-d01b-4b1f-9dab-2f3e40aff28b");
const nextSource = ownedSource(next, "563864ed-08b8-41eb-94c3-2948e17d0b0e");
const chosenSource = ownedSource(next, "9f3b6d52-6a2a-4f4a-9a55-5d3b0a9d8c11");
const afterSource = ownedSource(after, "2b77f558-ac64-4431-a097-9d141f80f0f9");
const waitingSource: Schema<"Source"> = {
  ...nextSource,
  ready: false,
  can_prepare: true,
};
const preparationId = "0d6a3f6b-0f0e-47f5-a1a5-3f1d3d9f9b10";
const preparationPath = `${sourcesPath(next)}/${nextSource.id}/preparation`;

afterEach(() => {
  vi.useRealTimers();
});

function status(
  phase: Schema<"SourcePreparationPhase">,
): Schema<"SourcePreparationStatus"> {
  return {
    preparation_id: preparationId,
    recording_id: next.id,
    source_id: nextSource.id,
    phase,
    created_at: 1,
    updated_at: 1,
  };
}

type Workflow = {
  nextSources: () => Schema<"Source">[];
  prepare?: (request: OwnedPlaybackRequest) => Response | Promise<Response>;
  poll?: (request: OwnedPlaybackRequest) => Response | Promise<Response>;
};

function workflow(handlers: Workflow) {
  return ownedPlaybackHttp((request) => {
    if (request.method === "GET") {
      if (request.path === cachedSourcesPath(first))
        return Response.json([firstSource]);
      if (request.path === cachedSourcesPath(next))
        return Response.json(handlers.nextSources());
      if (request.path === cachedSourcesPath(after))
        return Response.json([afterSource]);
      if (request.path === preparationPath && handlers.poll)
        return handlers.poll(request);
    }
    if (request.method === "POST") {
      if (request.path === `${sourcesPath(next)}/refresh`)
        return Response.json(handlers.nextSources());
      if (request.path === `${sourcesPath(next)}/resolve`) {
        const body = request.body as Schema<"ResolveSource">;
        return Response.json({
          request_id: body.request_id,
          resolution_id: "6065e385-46bb-47cc-ac68-2cadf61b6d7c",
          recording_id: next.id,
          outcome: "no_suitable_source",
          sources: handlers.nextSources(),
        } satisfies Schema<"SourceResolution">);
      }
      if (request.path === preparationPath && handlers.prepare)
        return handlers.prepare(request);
      if (request.path === "/api/v1/sync")
        return Response.json({
          acknowledged: (request.body as Schema<"SyncBatch">).operations.map(
            (operation) => operation.id,
          ),
        });
    }
    throw new Error("Unexpected deferred preparation workflow request");
  });
}

const accepted = (request: OwnedPlaybackRequest) =>
  Response.json(
    {
      preparation_id: preparationId,
      recording_id: next.id,
      source_id: nextSource.id,
      request_id: (request.body as { request_id: string }).request_id,
    } satisfies Schema<"SourcePreparationReceipt">,
    { status: 202 },
  );

const preparationRequests = (requests: OwnedPlaybackRequest[]) =>
  requests.filter((request) => request.path === preparationPath);

function watchPauses(element: HTMLAudioElement) {
  const pauses = vi.fn();
  element.addEventListener("pause", pauses);
  return pauses;
}

test("a next entry waiting for provider preparation is prepared and buffered during current playback, then adopted without a second search or download", async () => {
  const media = browserPreparationMedia();
  let prepared = false;
  const { requests, unexpected } = workflow({
    nextSources: () => [prepared ? nextSource : waitingSource],
    prepare: (request) => {
      prepared = true;
      return accepted(request);
    },
    poll: () => Response.json(status("ready")),
  });
  const { players, unmount } = renderPlaybackAccount("alice");
  const player = players.get("alice")!;
  try {
    await act(async () => {
      await player.playQueue([first, next, after]);
    });
    const current = media.created[0];
    const pauses = watchPauses(current);
    const buffered = await media.loadedSource(nextSource.id);
    await act(async () => {
      media.canPlay(buffered);
    });
    expect(preparationRequests(requests).map((r) => r.method)).toEqual([
      "POST",
      "GET",
    ]);
    expect(
      Object.keys(preparationRequests(requests)[0].body as object),
    ).toEqual(["request_id"]);
    expect(pauses).not.toHaveBeenCalled();
    expect(current.paused).toBe(false);
    expect(player.snapshot().current?.id).toBe(first.id);
    expect(player.snapshot().playing).toBe(true);
    expect(media.played.map((play) => play.source)).toEqual([
      audioPath(firstSource.id),
    ]);
    expect(buffered.paused).toBe(true);

    const checks = requests.length;
    const adopted = deferred<void>();
    const stop = player.subscribe(() => {
      if (
        player.snapshot().current?.id === next.id &&
        player.snapshot().playing
      )
        adopted.resolve();
    });
    await act(async () => {
      media.end(current);
      await adopted.promise;
    });
    stop();
    expect(media.played.map((play) => play.media)).toEqual([current, buffered]);
    expect(
      media.loaded.filter((load) => load.media === buffered && load.source),
    ).toHaveLength(1);
    expect(
      requests
        .slice(checks)
        .filter((request) => request.path.startsWith(sourcesPath(next))),
    ).toEqual([]);
    expect(
      requests.filter((request) => request.path.endsWith("/resolve")),
    ).toHaveLength(1);
    expect(
      preparationRequests(requests).filter((r) => r.method === "POST"),
    ).toHaveLength(1);
    expect(requests.some((request) => request.path.endsWith("/download"))).toBe(
      false,
    );
    expect(unexpected).toEqual([]);
  } finally {
    unmount();
    media.close();
  }
});

test("a preparing source is checked on the server's cadence while the current track keeps playing", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const media = browserPreparationMedia();
  let prepared = false;
  let phase: Schema<"SourcePreparationPhase"> = "preparing";
  const { requests } = workflow({
    nextSources: () => [prepared ? nextSource : waitingSource],
    prepare: (request) => {
      return accepted(request);
    },
    poll: () => {
      if (phase === "ready") prepared = true;
      return Response.json(status(phase));
    },
  });
  const { players, unmount } = renderPlaybackAccount("alice");
  const player = players.get("alice")!;
  try {
    await act(async () => {
      await player.playQueue([first, next]);
    });
    const current = media.created[0];
    const pauses = watchPauses(current);
    const checks = () =>
      preparationRequests(requests).filter((r) => r.method === "GET").length;
    await vi.waitFor(() => expect(checks()).toBe(1));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(4000);
    });
    expect(checks()).toBe(1);
    expect(media.created).toHaveLength(1);
    phase = "ready";
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1500);
    });
    const buffered = await media.loadedSource(nextSource.id);
    expect(checks()).toBe(2);
    expect(buffered).not.toBe(current);
    expect(pauses).not.toHaveBeenCalled();
    expect(current.paused).toBe(false);
  } finally {
    unmount();
    media.close();
  }
});

test.each([
  ["failed", "failed"],
  ["blocked", "blocked"],
  ["cancelled", "cancelled"],
  ["submission_unknown", "submission_unknown"],
] as const)(
  "a %s preparation leaves current playback alone and the boundary falls back honestly",
  async (_name, phase) => {
    const media = browserPreparationMedia();
    const { requests, unexpected } = workflow({
      nextSources: () => [waitingSource],
      prepare: accepted,
      poll: () => Response.json(status(phase)),
    });
    const { players, unmount } = renderPlaybackAccount("alice");
    const player = players.get("alice")!;
    try {
      await act(async () => {
        await player.playQueue([first, next, after]);
      });
      const current = media.created[0];
      await vi.waitFor(() =>
        expect(
          preparationRequests(requests).filter((r) => r.method === "GET"),
        ).toHaveLength(1),
      );
      await act(async () => {
        await Promise.resolve();
      });
      expect(media.created).toHaveLength(1);
      expect(current.paused).toBe(false);
      expect(player.snapshot().error).toBe("");
      expect(player.snapshot().current?.id).toBe(first.id);
      await act(async () => {
        media.end(current);
      });
      await vi.waitFor(() =>
        expect(player.snapshot().current?.id).toBe(after.id),
      );
      expect(screen.getByRole("status").textContent).toBe(
        `Audio is unavailable for ${next.title}.`,
      );
      expect(
        preparationRequests(requests).filter((r) => r.method === "POST"),
      ).toHaveLength(1);
      expect(unexpected).toEqual([]);
    } finally {
      unmount();
      media.close();
    }
  },
);

test("a rejected preparation request is not retried and a source that cannot be prepared is not requested", async () => {
  const media = browserPreparationMedia();
  const { requests } = workflow({
    nextSources: () => [waitingSource],
    prepare: () => Response.json({ error: "Quota reached" }, { status: 429 }),
  });
  const { players, unmount } = renderPlaybackAccount("alice");
  const player = players.get("alice")!;
  try {
    await act(async () => {
      await player.playQueue([first, next]);
    });
    await vi.waitFor(() =>
      expect(preparationRequests(requests)).toHaveLength(1),
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(preparationRequests(requests)).toHaveLength(1);
    expect(media.created).toHaveLength(1);
    expect(player.snapshot().error).toBe("");
    expect(player.snapshot().playing).toBe(true);
  } finally {
    unmount();
    media.close();
  }

  const plain = browserPreparationMedia();
  const second = workflow({
    nextSources: () => [{ ...waitingSource, can_prepare: false }],
    prepare: accepted,
  });
  const view = renderPlaybackAccount("alice");
  try {
    await act(async () => {
      await view.players.get("alice")!.playQueue([first, next]);
    });
    await vi.waitFor(() =>
      expect(
        second.requests.filter((r) => r.path.endsWith("/resolve")),
      ).toHaveLength(1),
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(preparationRequests(second.requests)).toEqual([]);
  } finally {
    view.unmount();
    plain.close();
  }
});

test("only the single upcoming entry is prepared however often position updates arrive", async () => {
  const media = browserPreparationMedia();
  const hold = deferred<Response>();
  const { requests } = workflow({
    nextSources: () => [waitingSource],
    prepare: accepted,
    poll: () => hold.promise,
  });
  const { players, unmount } = renderPlaybackAccount("alice");
  const player = players.get("alice")!;
  try {
    await act(async () => {
      await player.playQueue([first, next, after]);
    });
    const current = media.created[0];
    await vi.waitFor(() =>
      expect(
        preparationRequests(requests).filter((r) => r.method === "GET"),
      ).toHaveLength(1),
    );
    for (let tick = 1; tick <= 5; tick++)
      act(() => {
        current.currentTime = tick;
        current.dispatchEvent(new Event("timeupdate"));
      });
    expect(preparationRequests(requests)).toHaveLength(2);
    expect(requests.some((r) => r.path === cachedSourcesPath(after))).toBe(
      false,
    );
    expect(media.created).toHaveLength(1);
  } finally {
    hold.resolve(Response.json(status("preparing")));
    unmount();
    media.close();
  }
});

test.each(["skip", "queue edit", "account change"] as const)(
  "%s stops preparation checks and a late ready answer cannot buffer or alter playback",
  async (change) => {
    const media = browserPreparationMedia();
    const hold = deferred<Response>();
    let prepared = false;
    const { requests } = workflow({
      nextSources: () => [prepared ? nextSource : waitingSource],
      prepare: accepted,
      poll: () => hold.promise,
    });
    const { players, view, rerender, unmount } = renderPlaybackAccount("alice");
    const player = players.get("alice")!;
    try {
      await act(async () => {
        await player.playQueue([first, next, after]);
      });
      await vi.waitFor(() =>
        expect(
          preparationRequests(requests).filter((r) => r.method === "GET"),
        ).toHaveLength(1),
      );
      const pending = preparationRequests(requests)[1];
      expect(pending.signal.aborted).toBe(false);
      const entries = (await player.queuePage(0, 10)).entries;
      if (change === "skip")
        await act(async () => {
          await player.selectEntry(entries[2].id);
        });
      else if (change === "queue edit")
        act(() => player.removeEntry(entries[1].id));
      else rerender(view("bob"));
      expect(pending.signal.aborted).toBe(true);
      const count = requests.length;
      prepared = true;
      await act(async () => {
        hold.resolve(Response.json(status("ready")));
        await hold.promise;
      });
      expect(requests).toHaveLength(count);
      expect(
        media.loaded.some((l) => l.source === audioPath(nextSource.id)),
      ).toBe(false);
    } finally {
      hold.resolve(Response.json(status("ready")));
      unmount();
      media.close();
    }
  },
);

test("a manual choice made while preparation ran is used at the boundary instead of the prepared source", async () => {
  const media = browserPreparationMedia();
  let phase: "waiting" | "prepared" | "chosen" = "waiting";
  const { players, unmount } = (() => {
    workflow({
      nextSources: () =>
        phase === "waiting"
          ? [waitingSource]
          : phase === "prepared"
            ? [nextSource]
            : [chosenSource, nextSource],
      prepare: (request) => {
        phase = "prepared";
        return accepted(request);
      },
      poll: () => Response.json(status("ready")),
    });
    return renderPlaybackAccount("alice");
  })();
  const player = players.get("alice")!;
  try {
    await act(async () => {
      await player.playQueue([first, next]);
    });
    const current = media.created[0];
    const buffered = await media.loadedSource(nextSource.id);
    await act(async () => {
      media.canPlay(buffered);
    });
    phase = "chosen";
    await act(async () => {
      media.end(current);
    });
    await vi.waitFor(() => expect(player.snapshot().current?.id).toBe(next.id));
    await vi.waitFor(() =>
      expect(media.played.map((play) => play.source)).toEqual([
        audioPath(firstSource.id),
        audioPath(chosenSource.id),
      ]),
    );
    expect(buffered.isConnected).toBe(false);
  } finally {
    unmount();
    media.close();
  }
});

test.each([
  ["a source that becomes ready in time is adopted", true],
  [
    "a source still preparing is abandoned and the track is skipped as unavailable",
    false,
  ],
] as const)("at the boundary, %s", async (_name, readyInTime) => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const media = browserPreparationMedia();
  let prepared = false;
  let phase: Schema<"SourcePreparationPhase"> = "preparing";
  const { requests } = workflow({
    nextSources: () => [prepared ? nextSource : waitingSource],
    prepare: accepted,
    poll: () => {
      if (phase === "ready") prepared = true;
      return Response.json(status(phase));
    },
  });
  const { players, unmount } = renderPlaybackAccount("alice");
  const player = players.get("alice")!;
  try {
    await act(async () => {
      await player.playQueue([first, next, after]);
    });
    const current = media.created[0];
    await vi.waitFor(() =>
      expect(
        preparationRequests(requests).filter((r) => r.method === "GET"),
      ).toHaveLength(1),
    );
    const pending = preparationRequests(requests)[1];
    await act(async () => {
      media.end(current);
    });
    expect(player.snapshot().current?.id).toBe(next.id);
    expect(player.snapshot().loading).toBe(true);
    if (readyInTime) {
      phase = "ready";
      await act(async () => {
        await vi.advanceTimersByTimeAsync(5000);
      });
      const buffered = await media.loadedSource(nextSource.id);
      await act(async () => {
        media.canPlay(buffered);
      });
      await vi.waitFor(() => expect(player.snapshot().playing).toBe(true));
      expect(player.snapshot().current?.id).toBe(next.id);
      expect(media.played.at(-1)?.source).toBe(audioPath(nextSource.id));
    } else {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(10_000);
      });
      await vi.waitFor(() =>
        expect(player.snapshot().current?.id).toBe(after.id),
      );
      expect(pending.signal.aborted).toBe(true);
      expect(screen.getByRole("status").textContent).toBe(
        `Audio is unavailable for ${next.title}.`,
      );
    }
  } finally {
    unmount();
    media.close();
  }
});

const statusReads = (requests: OwnedPlaybackRequest[]) =>
  preparationRequests(requests).filter((request) => request.method === "GET");
const submissions = (requests: OwnedPlaybackRequest[]) =>
  preparationRequests(requests).filter((request) => request.method === "POST");

test("a boundary reached during a slow resolution waits at most ten seconds and then skips", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const media = browserPreparationMedia();
  const held = deferred<Response>();
  let resolving: OwnedPlaybackRequest | null = null;
  const { requests } = ownedPlaybackHttp((request) => {
    if (request.path === cachedSourcesPath(first) && request.method === "GET")
      return Response.json([firstSource]);
    if (request.path === cachedSourcesPath(after) && request.method === "GET")
      return Response.json([afterSource]);
    if (request.path === cachedSourcesPath(next) && request.method === "GET")
      return Response.json([]);
    if (request.path === `${sourcesPath(next)}/refresh`)
      return Response.json([]);
    if (request.path === `${sourcesPath(next)}/resolve`) {
      resolving = request;
      return held.promise;
    }
    if (request.path === "/api/v1/sync")
      return Response.json({
        acknowledged: (request.body as Schema<"SyncBatch">).operations.map(
          (operation) => operation.id,
        ),
      });
    throw new Error("Unexpected slow resolution request");
  });
  const { players, unmount } = renderPlaybackAccount("alice");
  const player = players.get("alice")!;
  try {
    await act(async () => {
      await player.playQueue([first, next, after]);
    });
    await vi.waitFor(() => expect(resolving).not.toBeNull());
    await act(async () => {
      media.end(media.created[0]);
    });
    expect(player.snapshot().current?.id).toBe(next.id);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(9_000);
    });
    expect(player.snapshot().current?.id).toBe(next.id);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_500);
    });
    await vi.waitFor(() =>
      expect(player.snapshot().current?.id).toBe(after.id),
    );
    expect(resolving!.signal.aborted).toBe(true);
    expect(requests.some((r) => r.path.endsWith("/preparation"))).toBe(false);
  } finally {
    held.resolve(Response.json({}));
    unmount();
    media.close();
  }
});

test.each([
  ["still preparing", () => Response.json(status("preparing"))],
  ["failed", () => Response.json(status("failed"))],
])(
  "pausing and resuming while the next source is %s never submits the same entry twice",
  async (_name, poll) => {
    const media = browserPreparationMedia();
    const { requests } = workflow({
      nextSources: () => [waitingSource],
      prepare: accepted,
      poll,
    });
    const { players, unmount } = renderPlaybackAccount("alice");
    const player = players.get("alice")!;
    try {
      await act(async () => {
        await player.playQueue([first, next]);
      });
      await vi.waitFor(() => expect(statusReads(requests)).toHaveLength(1));
      for (let toggle = 0; toggle < 2; toggle++) {
        await act(async () => {
          await player.setPlaying(false);
        });
        await act(async () => {
          await player.setPlaying(true);
        });
        await vi.waitFor(() =>
          expect(
            requests.filter((r) => r.path.endsWith("/resolve")).length,
          ).toBeGreaterThan(toggle + 1),
        );
      }
      await act(async () => {
        await Promise.resolve();
      });
      expect(submissions(requests)).toHaveLength(1);
    } finally {
      unmount();
      media.close();
    }
  },
);

test("a rejected submission is not repeated after pausing and resuming", async () => {
  const media = browserPreparationMedia();
  const { requests } = workflow({
    nextSources: () => [waitingSource],
    prepare: () => Response.json({ error: "Quota" }, { status: 429 }),
  });
  const { players, unmount } = renderPlaybackAccount("alice");
  const player = players.get("alice")!;
  try {
    await act(async () => {
      await player.playQueue([first, next]);
    });
    await vi.waitFor(() => expect(submissions(requests)).toHaveLength(1));
    await act(async () => {
      await player.setPlaying(false);
    });
    await act(async () => {
      await player.setPlaying(true);
    });
    await vi.waitFor(() =>
      expect(
        requests.filter((r) => r.path.endsWith("/resolve")).length,
      ).toBeGreaterThan(1),
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(submissions(requests)).toHaveLength(1);
  } finally {
    unmount();
    media.close();
  }
});

test.each([
  ["a preparing status", () => Response.json(status("preparing"))],
  ["an absent status", () => Response.json({ error: "none" }, { status: 404 })],
])(
  "polling %s stops at the ninety second bound and the entry falls back",
  async (_name, poll) => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const media = browserPreparationMedia();
    const { requests } = workflow({
      nextSources: () => [waitingSource],
      prepare: accepted,
      poll,
    });
    const { players, unmount } = renderPlaybackAccount("alice");
    const player = players.get("alice")!;
    try {
      await act(async () => {
        await player.playQueue([first, next, after]);
      });
      const current = media.created[0];
      await vi.waitFor(() => expect(statusReads(requests)).toHaveLength(1));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(80_000);
      });
      expect(statusReads(requests).length).toBeGreaterThan(10);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(11_000);
      });
      const reads = statusReads(requests);
      expect(reads.every((read) => read.signal.aborted)).toBe(true);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
      expect(statusReads(requests)).toHaveLength(reads.length);
      expect(submissions(requests)).toHaveLength(1);
      expect(player.snapshot().current?.id).toBe(first.id);
      expect(player.snapshot().playing).toBe(true);
      await act(async () => {
        media.end(current);
      });
      await vi.waitFor(() =>
        expect(player.snapshot().current?.id).toBe(after.id),
      );
      expect(statusReads(requests)).toHaveLength(reads.length);
      expect(
        media.loaded.some((load) => load.source === audioPath(nextSource.id)),
      ).toBe(false);
    } finally {
      unmount();
      media.close();
    }
  },
);

test("three consecutive failed status reads stop polling", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const media = browserPreparationMedia();
  const { requests } = workflow({
    nextSources: () => [waitingSource],
    prepare: accepted,
    poll: () => Response.json({ error: "down" }, { status: 503 }),
  });
  const { players, unmount } = renderPlaybackAccount("alice");
  const player = players.get("alice")!;
  try {
    await act(async () => {
      await player.playQueue([first, next]);
    });
    await vi.waitFor(() => expect(statusReads(requests)).toHaveLength(1));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(statusReads(requests)).toHaveLength(3);
    expect(player.snapshot().error).toBe("");
    expect(player.snapshot().playing).toBe(true);
  } finally {
    unmount();
    media.close();
  }
});

test("a next check beyond the lookahead deadline stops instead of polling", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const media = browserPreparationMedia();
  const { requests } = workflow({
    nextSources: () => [waitingSource],
    prepare: accepted,
    poll: () =>
      Response.json({
        ...status("preparing"),
        next_check_at: Math.floor(Date.now() / 1000) + 3600,
      }),
  });
  const { players, unmount } = renderPlaybackAccount("alice");
  const player = players.get("alice")!;
  try {
    await act(async () => {
      await player.playQueue([first, next]);
    });
    await vi.waitFor(() => expect(statusReads(requests)).toHaveLength(1));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(120_000);
    });
    expect(statusReads(requests)).toHaveLength(1);
    expect(player.snapshot().playing).toBe(true);
  } finally {
    unmount();
    media.close();
  }
});
