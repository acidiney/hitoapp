import type { Schema } from "../../../shared/schema";
import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { useDiscovery } from "../hooks/useDiscovery";

import { deferred } from "../../../test/async";

const connection: Schema<"PluginInstallation"> = {
  id: "connection",
  service_id: "service",
  revision: 2,
  checked_at: 1,
  credential_set: true,
  manifest: {
    id: "org.hito.fixture",
    name: "Fixture collection",
    protocol_version: 1,
    capabilities: ["search", "sources"],
  },
};
const hit = {
  id: "private/remote?id=1",
  title: "Private provider title",
  artist: "Private artist",
};
const recording: Schema<"Recording"> = {
  id: "durable-recording",
  library_id: "personal-library",
  title: "Personal title",
  artist: "Personal artist",
  inherited_title: hit.title,
  inherited_artist: hit.artist,
  duration_ms: 90000,
  available: true,
  liked: false,
};
const player = {
  prepareSelection: vi.fn(() => vi.fn().mockResolvedValue(true)),
};
const response = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
test("search uses the private POST contract and leaves transient hits out of playback", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValue(
      response({ items: [hit], next_cursor: "private-cursor" }),
    );
  vi.stubGlobal("fetch", fetch);
  const { result } = renderHook(() =>
    useDiscovery("alice", connection, player),
  );
  await act(async () => {
    await result.current.search("private query");
  });
  expect(result.current.items).toEqual([hit]);
  expect(result.current.error).toBe("");
  const [url, options] = fetch.mock.calls[0];
  expect(url).toBe("/api/v1/plugins/connection/search");
  expect(options.method).toBe("POST");
  expect(options.headers["x-hito-account"]).toBe("alice");
  expect(JSON.parse(options.body)).toEqual({
    installation_revision: 2,
    query: "private query",
    cursor: null,
    limit: 20,
  });
  expect(player.prepareSelection).not.toHaveBeenCalled();
});

test("a repeated search supersedes a delayed response even when transport ignores cancellation", async () => {
  const first = deferred<Response>();
  const fetch = vi
    .fn()
    .mockReturnValueOnce(first.promise)
    .mockResolvedValueOnce(response({ items: [{ ...hit, id: "new" }] }));
  vi.stubGlobal("fetch", fetch);
  const { result } = renderHook(() =>
    useDiscovery("alice", connection, player),
  );
  let pending!: Promise<boolean>;
  act(() => {
    pending = result.current.search("same query");
  });
  await act(async () => {
    await result.current.search("same query");
  });
  expect(fetch.mock.calls[0][1].signal.aborted).toBe(true);
  await act(async () => {
    first.resolve(response({ items: [hit] }));
    await pending;
  });
  expect(result.current.items.map((item) => item.id)).toEqual(["new"]);
});

test("cursor paging keeps one page and serializes duplicate navigation", async () => {
  const second = deferred<Response>();
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(
      response({ items: [hit], next_cursor: "private-next" }),
    )
    .mockReturnValueOnce(second.promise)
    .mockResolvedValueOnce(
      response({ items: [hit], next_cursor: "private-next" }),
    );
  vi.stubGlobal("fetch", fetch);
  const { result } = renderHook(() =>
    useDiscovery("alice", connection, player),
  );
  await act(async () => {
    await result.current.search("query");
  });
  let pending!: Promise<boolean>;
  act(() => {
    pending = result.current.next();
    void result.current.next();
  });
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(JSON.parse(fetch.mock.calls[1][1].body).cursor).toBe("private-next");
  await act(async () => {
    second.resolve(response({ items: [{ ...hit, id: "second" }] }));
    await pending;
  });
  expect(result.current.items.map((item) => item.id)).toEqual(["second"]);
  expect(result.current.page).toBe(1);
  expect(result.current.hasPrevious).toBe(true);
  await act(async () => {
    await result.current.previous();
  });
  expect(JSON.parse(fetch.mock.calls[2][1].body).cursor).toBeNull();
  expect(result.current.items).toEqual([hit]);
});

test("completed saves remain visible across paging and searches within the connection scope", async () => {
  const page = { items: [hit], next_cursor: "next" };
  const fetch = vi
    .fn()
    .mockImplementation(() => Promise.resolve(response(page)))
    .mockResolvedValueOnce(response(page))
    .mockResolvedValueOnce(response(recording))
    .mockResolvedValueOnce(response({ items: [{ ...hit, id: "second" }] }));
  vi.stubGlobal("fetch", fetch);
  const { result, rerender } = renderHook(
    (revision) => useDiscovery("alice", { ...connection, revision }, player),
    { initialProps: connection.revision },
  );
  await act(async () => {
    await result.current.search("query");
  });
  await act(async () => {
    await result.current.select(hit, "save");
  });
  expect(result.current.saved.get(hit.id)?.id).toBe(recording.id);
  await act(async () => {
    await result.current.next();
  });
  await act(async () => {
    await result.current.previous();
  });
  expect(result.current.items).toEqual([hit]);
  expect(result.current.saved.get(hit.id)?.id).toBe(recording.id);
  for (const query of ["query", "other query", "", "query"]) {
    await act(async () => {
      await result.current.search(query);
    });
    expect(result.current.saved.get(hit.id)?.id).toBe(recording.id);
  }
  rerender(connection.revision + 1);
  expect(result.current.saved.size).toBe(0);
  expect(player.prepareSelection).not.toHaveBeenCalled();
});

test("completed saved state never returns after account or connection authority changes", async () => {
  for (const changed of [
    { account: "bob", connection },
    { account: "alice", connection: { ...connection, id: "other" } },
    { account: "alice", connection: { ...connection, revision: 3 } },
  ]) {
    const fetch = vi
      .fn()
      .mockImplementation(() => Promise.resolve(response({ items: [hit] })))
      .mockResolvedValueOnce(response({ items: [hit] }))
      .mockResolvedValueOnce(response(recording));
    vi.stubGlobal("fetch", fetch);
    const original = { account: "alice", connection };
    const { result, rerender, unmount } = renderHook(
      (props) => useDiscovery(props.account, props.connection, player),
      { initialProps: original },
    );
    await act(async () => {
      await result.current.search("query");
    });
    await act(async () => {
      await result.current.select(hit, "save");
    });
    expect(result.current.saved.has(hit.id)).toBe(true);
    rerender(changed);
    expect(result.current.saved.size).toBe(0);
    rerender(original);
    await act(async () => {
      await result.current.search("query");
    });
    expect(result.current.saved.size).toBe(0);
    unmount();
  }
});

test("play waits for adoption and uses the authoritative durable recording", async () => {
  const adopted = deferred<Response>();
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(response({ items: [hit] }))
    .mockReturnValueOnce(adopted.promise);
  vi.stubGlobal("fetch", fetch);
  const play = vi.fn().mockResolvedValue(true);
  player.prepareSelection.mockReturnValue(play);
  const { result } = renderHook(() =>
    useDiscovery("alice", connection, player),
  );
  await act(async () => {
    await result.current.search("query");
  });
  let pending!: Promise<unknown>;
  act(() => {
    pending = result.current.select(hit, "play");
  });
  expect(play).not.toHaveBeenCalled();
  expect(fetch.mock.calls[1][0]).toBe("/api/v1/plugins/connection/recordings");
  expect(JSON.parse(fetch.mock.calls[1][1].body)).toEqual({
    installation_revision: 2,
    provider_recording_id: hit.id,
  });
  await act(async () => {
    adopted.resolve(response(recording));
    await pending;
  });
  expect(play).toHaveBeenCalledExactlyOnceWith(recording);
  expect(result.current.saved.get(hit.id)?.id).toBe(recording.id);
});

test("provider, account and revision changes discard private results and delayed adoption", async () => {
  for (const changed of [
    { account: "bob", connection },
    { account: "alice", connection: { ...connection, id: "other" } },
    { account: "alice", connection: { ...connection, revision: 3 } },
  ]) {
    const adopted = deferred<Response>();
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(response({ items: [hit] }))
      .mockReturnValueOnce(adopted.promise);
    vi.stubGlobal("fetch", fetch);
    const play = vi.fn().mockResolvedValue(true);
    player.prepareSelection.mockReturnValue(play);
    const { result, rerender, unmount } = renderHook(
      (props) => useDiscovery(props.account, props.connection, player),
      { initialProps: { account: "alice", connection } },
    );
    await act(async () => {
      await result.current.search("private query");
    });
    let pending!: Promise<unknown>;
    act(() => {
      pending = result.current.select(hit, "play");
    });
    rerender(changed);
    expect(result.current.items).toEqual([]);
    expect(result.current.selection).toBeNull();
    expect(fetch.mock.calls[1][1].signal.aborted).toBe(true);
    await act(async () => {
      adopted.resolve(response(recording));
      await pending;
    });
    expect(play).not.toHaveBeenCalled();
    expect(result.current.saved.size).toBe(0);
    unmount();
  }
});

test("saving never starts audio and unavailable playback remains a saved retryable recording", async () => {
  const unavailable = { ...recording, available: false };
  const fetch = vi
    .fn()
    .mockImplementation(() => Promise.resolve(response(unavailable)))
    .mockResolvedValueOnce(response({ items: [hit] }));
  vi.stubGlobal("fetch", fetch);
  const play = vi.fn().mockResolvedValue(true);
  player.prepareSelection.mockReturnValue(play);
  const { result } = renderHook(() =>
    useDiscovery("alice", connection, player),
  );
  await act(async () => {
    await result.current.search("query");
  });
  await act(async () => {
    await result.current.select(hit, "save");
  });
  expect(player.prepareSelection).not.toHaveBeenCalled();
  expect(result.current.saved.get(hit.id)).toEqual(unavailable);
  await act(async () => {
    await result.current.select(hit, "play");
  });
  expect(result.current.selection?.error).toContain("No source is ready");
  expect(play).not.toHaveBeenCalled();
  expect(fetch).toHaveBeenCalledTimes(3);
});

test("account changes also reject selection completion while playback is still resolving", async () => {
  const playing = deferred<boolean>();
  const play = vi.fn().mockReturnValue(playing.promise);
  player.prepareSelection.mockReturnValue(play);
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValueOnce(response({ items: [hit] }))
      .mockResolvedValueOnce(response(recording)),
  );
  const { result, rerender } = renderHook(
    ({ account }) => useDiscovery(account, connection, player),
    { initialProps: { account: "alice" } },
  );
  await act(async () => {
    await result.current.search("query");
  });
  let pending!: Promise<unknown>;
  act(() => {
    pending = result.current.select(hit, "play");
  });
  await waitFor(() => expect(play).toHaveBeenCalled());
  rerender({ account: "bob" });
  let completed: unknown;
  await act(async () => {
    playing.resolve(true);
    completed = await pending;
  });
  expect(completed).toBeNull();
  expect(result.current.saved.size).toBe(0);
});

test("a delayed page cannot replace a newer search and cursor cycles stop paging", async () => {
  const old = deferred<Response>();
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(response({ items: [hit], next_cursor: "next" }))
    .mockReturnValueOnce(old.promise)
    .mockResolvedValueOnce(
      response({ items: [{ ...hit, id: "fresh" }], next_cursor: "cycle" }),
    )
    .mockResolvedValueOnce(
      response({ items: [{ ...hit, id: "cycled" }], next_cursor: "cycle" }),
    );
  vi.stubGlobal("fetch", fetch);
  const { result } = renderHook(() =>
    useDiscovery("alice", connection, player),
  );
  await act(async () => {
    await result.current.search("first");
  });
  let pending!: Promise<boolean>;
  act(() => {
    pending = result.current.next();
  });
  await act(async () => {
    await result.current.search("fresh");
    old.resolve(response({ items: [hit] }));
    await pending;
  });
  expect(result.current.items.map((item) => item.id)).toEqual(["fresh"]);
  await act(async () => {
    await result.current.next();
  });
  expect(result.current.hasNext).toBe(false);
  expect(result.current.error).toContain("repeated");
});

test("logs exclude queries, cursors, provider content, and rejected private errors", async () => {
  const info = vi.spyOn(console, "info").mockImplementation(() => {});
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValueOnce(
        response({ items: [hit], next_cursor: "private-page-cursor" }),
      )
      .mockResolvedValueOnce(
        response({ error: "private-rejected-detail" }, 502),
      ),
  );
  const { result } = renderHook(() =>
    useDiscovery("alice", connection, player),
  );
  await act(async () => {
    await result.current.search("private-search-query");
  });
  await act(async () => {
    await result.current.select(hit, "save");
  });
  expect(result.current.selection?.error).toBe("private-rejected-detail");
  const logged = JSON.stringify([...info.mock.calls, ...warn.mock.calls]);
  for (const value of [
    hit.id,
    hit.title,
    hit.artist,
    "private-search-query",
    "private-page-cursor",
    "private-rejected-detail",
  ])
    expect(logged).not.toContain(value);
  expect(logged).toContain("providers.adopt");
});

test("a service without sources still saves results but never starts playback from them", async () => {
  const searchOnly: Schema<"PluginInstallation"> = {
    ...connection,
    manifest: {
      ...connection.manifest,
      capabilities: ["search", "metadata", "artwork"],
    },
  };
  const unavailable = { ...recording, available: false };
  const fetch = vi
    .fn()
    .mockImplementation(() => Promise.resolve(response(unavailable)))
    .mockResolvedValueOnce(response({ items: [hit] }));
  vi.stubGlobal("fetch", fetch);
  player.prepareSelection.mockClear();
  const { result } = renderHook(() =>
    useDiscovery("alice", searchOnly, player),
  );
  await act(async () => {
    await result.current.search("query");
  });
  await act(async () => {
    await result.current.select(hit, "play");
  });
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(player.prepareSelection).not.toHaveBeenCalled();
  await act(async () => {
    await result.current.select(hit, "save");
  });
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(result.current.saved.get(hit.id)).toEqual(unavailable);
  expect(player.prepareSelection).not.toHaveBeenCalled();
});
