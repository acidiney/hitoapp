import { act, renderHook } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { useSourcePreparation } from "./hooks/useSourcePreparation";
import type { Source, SourcePreparationStatus } from "./types";

const recording = "bd22c4d8-c9cc-42c1-9cb7-57c729c69653";
const source: Source = {
  id: "b98cf49b-d617-415c-93c3-2a2f9955c779",
  recording_id: recording,
  provider: "plugin",
  device_id: null,
  label: "Owned audio",
  ready: false,
  can_prepare: true,
};
const preparation = "fdd0f89b-a58c-4c08-a63a-16139c9af31a";
const other = "60055886-325b-49e5-8326-9f237c6111ef";
const initial = {
  account: "alice",
  recording,
  source: source as Source | null,
};

function status(
  update: Partial<SourcePreparationStatus> = {},
): SourcePreparationStatus {
  return {
    preparation_id: preparation,
    recording_id: recording,
    source_id: source.id,
    phase: "preparing",
    progress_basis_points: 2500,
    eta_seconds: null,
    reason: null,
    next_check_at: null,
    created_at: 1,
    updated_at: 1,
    ...update,
  };
}

function heldResponse() {
  let resolve!: (response: Response) => void;
  let reject!: (error: Error) => void;
  const response = new Promise<Response>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { response, resolve, reject };
}

test.each(["account", "recording", "source", "removal", "unmount"])(
  "%s teardown aborts a pending write and ignores its late acknowledgement",
  async (change) => {
    vi.useFakeTimers();
    const pending = heldResponse();
    const confirm = vi.fn(async () => true);
    const fetch = vi.fn(async (_url: string, options?: RequestInit) =>
      options?.method === "POST"
        ? pending.response
        : new Response(null, { status: 404 }),
    );
    vi.stubGlobal("fetch", fetch);
    const { result, rerender, unmount } = renderHook(
      (props) =>
        useSourcePreparation(
          props.account,
          props.recording,
          props.source,
          confirm,
        ),
      { initialProps: initial },
    );
    await act(async () => {});
    act(() => {
      void result.current.start();
      void result.current.start();
    });
    expect(
      fetch.mock.calls.filter(([, options]) => options?.method === "POST"),
    ).toHaveLength(1);
    const write = fetch.mock.calls.find(
      ([, options]) => options?.method === "POST",
    )!;
    const body = JSON.parse(String(write[1]?.body));
    const signal = write[1]?.signal;
    if (change === "unmount") unmount();
    else
      rerender({
        account: change === "account" ? "bob" : "alice",
        recording: change === "recording" ? other : recording,
        source:
          change === "removal"
            ? null
            : { ...source, id: change === "source" ? other : source.id },
      });
    expect(signal?.aborted).toBe(true);
    await act(async () => {
      pending.resolve(
        Response.json(
          {
            request_id: body.request_id,
            preparation_id: preparation,
            recording_id: recording,
            source_id: source.id,
          },
          { status: 202 },
        ),
      );
    });
    const after = fetch.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(120_000);
    });
    expect(fetch).toHaveBeenCalledTimes(after);
    expect(confirm).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
    if (change !== "unmount") {
      expect(result.current.receipt).toBeNull();
      expect(result.current.attempt).toBeNull();
      expect(result.current.status).toBeNull();
      expect(result.current.error).toBe("");
      unmount();
    }
  },
);

test.each(["success", "failure"])(
  "an old read's late %s cannot cross source ownership",
  async (completion) => {
    vi.useFakeTimers();
    const pending = heldResponse();
    const confirm = vi.fn(async () => true);
    const fetch = vi.fn(
      async (_url: string, _options?: RequestInit) => pending.response,
    );
    fetch
      .mockImplementationOnce(async () => pending.response)
      .mockImplementation(async () => new Response(null, { status: 404 }));
    vi.stubGlobal("fetch", fetch);
    const { result, rerender, unmount } = renderHook(
      (props) =>
        useSourcePreparation(
          props.account,
          props.recording,
          props.source,
          confirm,
        ),
      { initialProps: initial },
    );
    const signal = fetch.mock.calls[0][1]?.signal;
    rerender({ ...initial, source: { ...source, id: other } });
    expect(signal?.aborted).toBe(true);
    await act(async () => {
      if (completion === "success")
        pending.resolve(Response.json(status({ phase: "ready" })));
      else pending.reject(new Error("private obsolete preparation diagnostic"));
    });
    expect(result.current.status).toBeNull();
    expect(result.current.statusError).toBe("");
    expect(confirm).not.toHaveBeenCalled();
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  },
);

test("an acknowledged identity is pinned and a different latest preparation stops observation", async () => {
  vi.useFakeTimers();
  let accepted = false;
  let newer = false;
  const confirm = vi.fn(async () => true);
  const fetch = vi.fn(async (_url: string, options?: RequestInit) => {
    if (options?.method === "POST") {
      accepted = true;
      return Response.json(
        {
          request_id: JSON.parse(String(options.body)).request_id,
          preparation_id: preparation,
          recording_id: recording,
          source_id: source.id,
        },
        { status: 202 },
      );
    }
    return accepted
      ? Response.json(status({ preparation_id: newer ? other : preparation }))
      : new Response(null, { status: 404 });
  });
  vi.stubGlobal("fetch", fetch);
  const { result, unmount } = renderHook(() =>
    useSourcePreparation("alice", recording, source, confirm),
  );
  await act(async () => {});
  await act(async () => {
    await result.current.start();
  });
  expect(result.current.receipt?.preparation_id).toBe(preparation);
  newer = true;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5_000);
  });
  expect(result.current.halted).toBe(true);
  expect(result.current.status).toBeNull();
  expect(result.current.receipt?.preparation_id).toBe(preparation);
  expect(result.current.statusError).toBe(
    "This source has a different preparation. Check audio availability again.",
  );
  const count = fetch.mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(120_000);
    await result.current.check();
  });
  expect(fetch).toHaveBeenCalledTimes(count);
  expect(confirm).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
  unmount();
});

test.each(["cancelled", "submission_unknown", "blocked", "ready"] as const)(
  "%s status stops unscheduled observation without submitting more work",
  async (phase) => {
    vi.useFakeTimers();
    const confirm = vi.fn(async () => true);
    const fetch = vi.fn(async () => Response.json(status({ phase })));
    vi.stubGlobal("fetch", fetch);
    const { result, unmount } = renderHook(() =>
      useSourcePreparation(
        "alice",
        recording,
        phase === "ready"
          ? { ...source, ready: true, can_prepare: false }
          : source,
        confirm,
      ),
    );
    await act(async () => {});
    expect(result.current.status?.phase).toBe(phase);
    expect(result.current.canStart).toBe(false);
    expect(confirm).toHaveBeenCalledTimes(phase === "ready" ? 1 : 0);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(120_000);
      await result.current.start();
    });
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
    unmount();
  },
);

test("revoked status authority stops polling with controlled copy", async () => {
  vi.useFakeTimers();
  const fetch = vi.fn(async () =>
    Response.json({ error: "private access diagnostic" }, { status: 403 }),
  );
  vi.stubGlobal("fetch", fetch);
  const { result, unmount } = renderHook(() =>
    useSourcePreparation("alice", recording, source, async () => true),
  );
  await act(async () => {});
  expect(result.current.halted).toBe(true);
  expect(result.current.statusError).toBe(
    "This source preparation is no longer available. Check audio availability again.",
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(120_000);
  });
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(vi.getTimerCount()).toBe(0);
  unmount();
});

test("status reads never overlap, wait at least five seconds and slow when progress is unchanged", async () => {
  vi.useFakeTimers();
  const pending = heldResponse();
  const fetch = vi.fn(async () => Response.json(status()));
  fetch.mockImplementationOnce(async () => pending.response);
  vi.stubGlobal("fetch", fetch);
  const { unmount } = renderHook(() =>
    useSourcePreparation("alice", recording, source, async () => true),
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve(Response.json(status()));
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(4_999);
  });
  expect(fetch).toHaveBeenCalledTimes(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
  expect(fetch).toHaveBeenCalledTimes(2);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(25_000);
  });
  const afterFast = fetch.mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch.mock.calls.length - afterFast).toBe(2);
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});

test("a future check hint delays polling and transport errors use bounded backoff", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(1_700_000_000_000);
  let failing = false;
  const times: number[] = [];
  const fetch = vi.fn(async () => {
    times.push(Date.now());
    if (failing) throw new TypeError("private unavailable service diagnostic");
    return Response.json(status({ next_check_at: 1_700_000_120 }));
  });
  vi.stubGlobal("fetch", fetch);
  const { result, unmount } = renderHook(() =>
    useSourcePreparation("alice", recording, source, async () => true),
  );
  await act(async () => {});
  await act(async () => {
    await vi.advanceTimersByTimeAsync(119_999);
  });
  expect(fetch).toHaveBeenCalledTimes(1);
  failing = true;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
  expect(fetch).toHaveBeenCalledTimes(2);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(380_000);
  });
  expect(times.slice(1).map((time) => time - times[1])).toEqual([
    0, 5_000, 20_000, 80_000, 380_000,
  ]);
  expect(result.current.statusError).toBe(
    "Hito could not check preparation. Try again.",
  );
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});

test.each(["blocked", "submission_unknown"] as const)(
  "scheduled %s status remains observable without another submission",
  async (phase) => {
    vi.useFakeTimers();
    vi.setSystemTime(1_700_000_000_000);
    const fetch = vi.fn(async (_url: string, _options?: RequestInit) =>
      Response.json(status({ phase, next_check_at: 1_700_000_010 })),
    );
    vi.stubGlobal("fetch", fetch);
    const { unmount } = renderHook(() =>
      useSourcePreparation("alice", recording, source, async () => true),
    );
    await act(async () => {});
    await act(async () => {
      await vi.advanceTimersByTimeAsync(9_999);
    });
    expect(fetch).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(
      fetch.mock.calls.every(([, options]) => options?.method === "GET"),
    ).toBe(true);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  },
);

test.each([
  { progress_basis_points: 10_001 },
  { eta_seconds: 604_801 },
  { source_id: other },
  { recording_id: other },
  { phase: "private invalid phase" },
  { reason: "private invalid diagnostic" },
])("invalid status is rejected before publication", async (invalid) => {
  vi.useFakeTimers();
  const fetch = vi.fn(async () => Response.json({ ...status(), ...invalid }));
  vi.stubGlobal("fetch", fetch);
  const confirm = vi.fn(async () => true);
  const { result, unmount } = renderHook(() =>
    useSourcePreparation("alice", recording, source, confirm),
  );
  await act(async () => {});
  expect(result.current.status).toBeNull();
  expect(result.current.statusError).toBe(
    "Hito could not check preparation. Try again.",
  );
  expect(confirm).not.toHaveBeenCalled();
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});
