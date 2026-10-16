import { afterEach, expect, test, vi } from "vitest";
import { NativeSession } from "./Session";
import { nativeService } from "./fixtures/service";

afterEach(() => vi.unstubAllGlobals());

test("a lost command response retries its exact payload before a later relative command", async () => {
  const commands: Record<string, unknown>[] = [];
  let release!: () => void;
  const blocked = new Promise<void>((resolve) => {
    release = resolve;
  });
  let attempts = 0;
  let closed = 0;
  const state = {
    revision: 0,
    selection: 0,
    queue_len: 0,
    can_previous: false,
    shuffle: false,
    repeat: "off",
    current_index: null,
    current: null,
    source_id: null,
    desired_playing: false,
    playing: false,
    seeking: false,
    volume: 1000,
    phase: "idle",
    error: null,
    last_failure: null,
    persistence_error: null,
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, options: RequestInit) => {
      expect(options.headers).toMatchObject({
        "x-hito-account": "alice",
        "x-hito-request": "1",
      });
      if (url === "/api/v1/player")
        return Response.json({ native: true, epoch: "epoch", state: null });
      if (url.endsWith("/claim"))
        return Response.json({
          native: true,
          epoch: "attached",
          lease: "private-lease",
          state,
        });
      if (url.endsWith("/events")) {
        return new Response(
          new ReadableStream({
            start(controller) {
              options.signal?.addEventListener("abort", () => {
                closed++;
                controller.error(new DOMException("Aborted", "AbortError"));
              });
            },
          }),
          { headers: { "content-type": "text/event-stream" } },
        );
      }
      if (url.endsWith("/commands")) {
        const command = JSON.parse(String(options.body));
        commands.push(command);
        if (attempts++ === 0) {
          await blocked;
          throw new TypeError("response lost");
        }
        return Response.json({
          id: command.id,
          sequence: command.sequence,
          revision: commands.length === 2 ? 1 : 2,
        });
      }
      throw new Error("Unexpected fixture request");
    }),
  );
  const session = new NativeSession("alice", {
    state() {},
    position() {},
    failure() {},
  });
  try {
    await session.open();
    const first = session.submit([
      { kind: "replace", recordings: ["recording"], start: 0 },
    ]);
    const next = session.submit([{ kind: "next" }]);
    await vi.waitFor(() => expect(commands).toHaveLength(1));
    release();
    await Promise.all([first, next]);
    expect(commands).toHaveLength(3);
    expect(commands[1]).toEqual(commands[0]);
    expect(commands[2]).toMatchObject({
      sequence: 2,
      revision: 1,
      action: { kind: "next" },
    });
    expect(commands[2].id).not.toBe(commands[0].id);
  } finally {
    session.close();
  }
  expect(closed).toBe(2);
});

test("queued intentions freeze their payload and admission remains bounded behind a blocked receipt", async () => {
  const service = nativeService();
  let release!: () => void;
  const blocked = new Promise<void>((resolve) => {
    release = resolve;
  });
  service.command = async (command) => {
    if (command.sequence === 1) await blocked;
    return Response.json({
      id: command.id,
      sequence: command.sequence,
      revision: command.revision,
    });
  };
  const session = new NativeSession("alice", {
    state() {},
    position() {},
    failure() {},
  });
  try {
    await session.open();
    const first = session.submit([{ kind: "next" }]);
    const recordings = ["original"];
    const second = session.submit([{ kind: "replace", recordings, start: 0 }]);
    const remaining = Array.from({ length: 30 }, () =>
      session.submit([{ kind: "next" }]),
    );
    await expect(session.submit([{ kind: "clear" }])).rejects.toThrow(
      "unavailable",
    );
    await vi.waitFor(() => expect(service.commands).toHaveLength(1));
    recordings[0] = "changed-after-intent";
    release();
    await Promise.all([first, second, ...remaining]);
    expect(service.commands).toHaveLength(32);
    expect(service.commands[1].action).toEqual({
      kind: "replace",
      recordings: ["original"],
      start: 0,
    });
  } finally {
    release();
    session.close();
  }
});
