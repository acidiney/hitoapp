import { vi } from "vitest";
import type { Command, Snapshot } from "../api";

export function nativeService() {
  const state: Snapshot = {
    revision: 0,
    selection: 0,
    queue_len: 0,
    current_index: null,
    current: null,
    can_previous: false,
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
  const streams = new Map<
    string,
    ReadableStreamDefaultController<Uint8Array>
  >();
  const service = {
    state,
    claims: 0,
    subscriptions: { state: 0, position: 0 },
    commands: [] as Command[],
    requests: [] as string[],
    command: async (command: Command) =>
      Response.json({
        id: command.id,
        sequence: command.sequence,
        revision: command.revision,
      }),
    queue: async (query: { revision: number; offset: number; limit: number }) =>
      Response.json({
        revision: query.revision,
        offset: query.offset,
        total: service.state.queue_len,
        entries: Array.from(
          {
            length: Math.min(
              query.limit,
              Math.max(0, service.state.queue_len - query.offset),
            ),
          },
          (_, i) => ({
            id: `entry-${query.offset + i}`,
            recording: {
              id: `recording-${query.offset + i}`,
              title: `Track ${query.offset + i}`,
              artist: "Fixture",
              duration_ms: 10000,
              available: true,
            },
          }),
        ),
      }),
    end(kind: "state" | "position") {
      streams.get(kind)!.close();
      streams.delete(kind);
    },
    emit(kind: string, value: unknown) {
      streams
        .get(kind)!
        .enqueue(
          new TextEncoder().encode(`data: ${JSON.stringify(value)}\n\n`),
        );
    },
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, options: RequestInit) => {
      service.requests.push(url);
      if (url === "/api/v1/player")
        return Response.json({
          native: true,
          epoch: `epoch-${service.claims}`,
          state: null,
        });
      if (url.endsWith("/claim"))
        return Response.json({
          native: true,
          epoch: `epoch-${++service.claims}`,
          lease: `lease-${service.claims}`,
          state: service.state,
        });
      if (url.endsWith("/commands")) {
        const command: Command = JSON.parse(String(options.body));
        service.commands.push(command);
        return service.command(command);
      }
      if (url.endsWith("/events"))
        return new Response(
          new ReadableStream({
            start(controller) {
              const kind = url.includes("/position/") ? "position" : "state";
              streams.set(kind, controller);
              service.subscriptions[kind]++;
              service.emit(
                kind,
                kind === "state"
                  ? service.state
                  : {
                      selection: service.state.selection,
                      entry: service.state.current?.id ?? null,
                      position_ms: 0,
                      duration_ms: null,
                      seek_id: null,
                    },
              );
              options.signal?.addEventListener(
                "abort",
                () => {
                  if (streams.get(kind) === controller) {
                    controller.close();
                    streams.delete(kind);
                  }
                },
                { once: true },
              );
            },
          }),
          { headers: { "content-type": "text/event-stream" } },
        );
      if (url.endsWith("/queue")) {
        const query = JSON.parse(String(options.body));
        return service.queue(query);
      }
      throw new Error("Unexpected native fixture request");
    }),
  );
  return service;
}
