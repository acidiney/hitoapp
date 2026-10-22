import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import SourceAvailability from "../components/SourceAvailability";
import type { Source, SourcePreparationStatus } from "../../../shared/types";
import { heldResponse } from "../../../test/async";

const player = vi.hoisted(() => ({ playQueue: vi.fn() }));
vi.mock("../../../player/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("@inertiajs/react", () => ({ Link: "a" }));

const recording = "8bd366c9-19c8-4c24-aef9-2d5a70b00d08";
const sourceId = "a40ffb4c-afb4-4aa5-a28f-e4f0be2ec9f1";
const preparation = "e930d518-a3b6-427a-bf8c-2316b95d9901";
const sourcesPath = `/api/v1/recordings/${recording}/sources`;
const preparationPath = `${sourcesPath}/${sourceId}/preparation`;
const track = {
  id: recording,
  title: "Owned track",
  artist: "Owned artist",
  duration_ms: 10_000,
  available: false,
};
const source: Source = {
  id: sourceId,
  recording_id: recording,
  provider: "plugin",
  device_id: null,
  ready: false,
  label: "Owned exact audio",
  can_prepare: true,
};

function status(
  phase: SourcePreparationStatus["phase"],
): SourcePreparationStatus {
  return {
    preparation_id: preparation,
    recording_id: recording,
    source_id: sourceId,
    phase,
    progress_basis_points: phase === "preparing" ? 2500 : null,
    eta_seconds: null,
    reason: null,
    next_check_at: null,
    created_at: 1,
    updated_at: 1,
  };
}

test.each([
  "network",
  "server",
  "wrong_request",
  "wrong_source",
  "wrong_recording",
  "empty_preparation",
  "wrong_status",
])(
  "an unconfirmed %s acknowledgement preserves one exact request through refresh and double clicks",
  async (failure) => {
    vi.useFakeTimers();
    const warnings = vi.spyOn(console, "warn").mockImplementation(() => {});
    const recovery = heldResponse();
    const refresh = heldResponse();
    let writes = 0;
    let checks = 0;
    let submitted = false;
    const fetch = vi.fn(
      async (url: string, options?: RequestInit): Promise<Response> => {
        if (url === `${sourcesPath}/refresh`) {
          checks++;
          return checks === 1 ? Response.json([source]) : refresh.response;
        }
        if (url === preparationPath && options?.method === "GET")
          return submitted
            ? Response.json(status("preparing"))
            : new Response(null, { status: 404 });
        if (url === preparationPath && options?.method === "POST") {
          writes++;
          submitted = true;
          if (writes > 1) return recovery.response;
          const body = JSON.parse(String(options.body));
          if (failure === "network")
            throw new TypeError("private acknowledgement diagnostics");
          if (failure === "server")
            return Response.json(
              { error: "private provider diagnostics" },
              { status: 500 },
            );
          return Response.json(
            {
              request_id:
                failure === "wrong_request" ? preparation : body.request_id,
              preparation_id:
                failure === "empty_preparation"
                  ? "00000000-0000-0000-0000-000000000000"
                  : preparation,
              recording_id:
                failure === "wrong_recording" ? preparation : recording,
              source_id: failure === "wrong_source" ? preparation : sourceId,
            },
            { status: failure === "wrong_status" ? 200 : 202 },
          );
        }
        throw new Error("Unexpected preparation request");
      },
    );
    vi.stubGlobal("fetch", fetch);
    const view = render(
      createElement(SourceAvailability, { account: "alice", track }),
    );
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "Check availability" }),
      );
    });
    const prepare = screen.getByRole("button", { name: "Prepare audio" });
    await act(async () => {
      fireEvent.click(prepare);
      fireEvent.click(prepare);
    });
    expect(writes).toBe(1);
    const first = fetch.mock.calls.find(
      ([, options]) => options?.method === "POST" && options.body !== undefined,
    )!;
    const body = String(first[1]?.body);
    const request = JSON.parse(body);
    expect(request).toEqual({
      request_id: expect.stringMatching(
        /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
      ),
    });
    expect(first[1]).toMatchObject({
      credentials: "same-origin",
      headers: { "x-hito-account": "alice", "x-hito-request": "1" },
      signal: expect.any(AbortSignal),
    });
    expect(
      screen.getByRole("button", { name: "Retry preparation request" }),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "Hito could not confirm the preparation request. Retry the same request to recover its result.",
      ),
    ).toBeTruthy();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    });
    view.rerender(
      createElement(SourceAvailability, {
        account: "alice",
        track: { ...track, title: "Edited owned title" },
      }),
    );
    expect(screen.getByText(source.label)).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Retry preparation request" }),
    ).toBeTruthy();
    await act(async () => {
      refresh.resolve(
        Response.json(
          { error: "private refresh diagnostics" },
          { status: 503 },
        ),
      );
    });
    const before = fetch.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(fetch).toHaveBeenCalledTimes(before);
    expect(screen.getByText(source.label)).toBeTruthy();
    const retry = screen.getByRole("button", {
      name: "Retry preparation request",
    });
    act(() => {
      fireEvent.click(retry);
      fireEvent.click(retry);
    });
    expect(writes).toBe(2);
    const writesOnly = fetch.mock.calls.filter(
      ([url, options]) => url === preparationPath && options?.method === "POST",
    );
    expect(writesOnly[1][1]?.body).toBe(body);
    expect((retry as HTMLButtonElement).disabled).toBe(true);
    await act(async () => {
      recovery.resolve(
        Response.json(
          {
            request_id: request.request_id,
            preparation_id: preparation,
            recording_id: recording,
            source_id: sourceId,
          },
          { status: 202 },
        ),
      );
    });
    expect(screen.getByText("Preparing audio…")).toBeTruthy();
    expect(screen.getByRole("progressbar")).toHaveProperty("value", 2500);
    expect(
      screen.queryByRole("button", { name: "Retry preparation request" }),
    ).toBeNull();
    expect(player.playQueue).not.toHaveBeenCalled();
    expect(
      fetch.mock.calls.every(
        ([url]) => !url.includes("/download") && !url.includes("/audio"),
      ),
    ).toBe(true);
    expect(document.body.textContent).not.toContain("private");
    expect(JSON.stringify(warnings.mock.calls)).not.toContain("private");
  },
);

test("terminal ready reads the same source once, leaves Play explicit, and a later unavailable snapshot removes Play", async () => {
  vi.useFakeTimers();
  const confirmation = heldResponse();
  let prepared = false;
  let sourceReads = 0;
  const fetch = vi.fn(
    async (url: string, options?: RequestInit): Promise<Response> => {
      if (url === `${sourcesPath}/refresh`)
        return Response.json([{ ...source, ready: false, can_prepare: true }]);
      if (url === preparationPath && options?.method === "GET")
        return Response.json(status(prepared ? "ready" : "preparing"));
      if (url === sourcesPath && options?.method === "GET") {
        sourceReads++;
        return confirmation.response;
      }
      throw new Error("Unexpected preparation request");
    },
  );
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(SourceAvailability, { account: "alice", track }),
  );
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
  });
  expect(screen.getByText("Preparing audio…")).toBeTruthy();
  prepared = true;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5_000);
  });
  expect(sourceReads).toBe(1);
  expect(screen.queryByRole("button", { name: "Play track" })).toBeNull();
  await act(async () => {
    confirmation.resolve(
      Response.json([{ ...source, ready: true, can_prepare: false }]),
    );
  });
  expect(screen.getByText("Audio is ready to play.")).toBeTruthy();
  const before = fetch.mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(120_000);
  });
  expect(fetch).toHaveBeenCalledTimes(before);
  expect(sourceReads).toBe(1);
  expect(player.playQueue).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Play track" }));
  expect(player.playQueue).toHaveBeenCalledWith([
    { ...track, available: true },
  ]);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
  });
  expect(screen.getByText("No audio is ready yet.")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Play track" })).toBeNull();
  expect(sourceReads).toBe(1);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method === "POST")
      .every(([url]) => url.endsWith("/sources/refresh")),
  ).toBe(true);
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
});

test.each(["refresh", "reopen"])(
  "a ready preparation permits a fresh explicit request after the same source becomes unavailable through %s",
  async (route) => {
    vi.useFakeTimers();
    const replacement = "a6de5503-5631-4dd6-9eb2-d8835a55af70";
    const requests: string[] = [];
    let unavailable = false;
    const fetch = vi.fn(async (url: string, options?: RequestInit) => {
      const ready = requests.length > 0 && !unavailable;
      const snapshot = { ...source, ready, can_prepare: !ready };
      if (url === `${sourcesPath}/refresh` || url === sourcesPath)
        return Response.json([snapshot]);
      if (url === preparationPath && options?.method === "POST") {
        requests.push(JSON.parse(String(options.body)).request_id);
        return Response.json(
          {
            request_id: requests[requests.length - 1],
            preparation_id: requests.length === 1 ? preparation : replacement,
            recording_id: recording,
            source_id: sourceId,
          },
          { status: 202 },
        );
      }
      if (url === preparationPath && options?.method === "GET") {
        if (!requests.length) return new Response(null, { status: 404 });
        return Response.json(
          requests.length === 1
            ? status("ready")
            : { ...status("preparing"), preparation_id: replacement },
        );
      }
      throw new Error("Unexpected repeated preparation request");
    });
    vi.stubGlobal("fetch", fetch);
    let view = render(
      createElement(SourceAvailability, { account: "alice", track }),
    );
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "Check availability" }),
      );
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Prepare audio" }));
    });
    expect(screen.getByText("Audio is ready to play.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Prepare audio" })).toBeNull();
    expect(requests).toHaveLength(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(120_000);
    });
    expect(requests).toHaveLength(1);

    unavailable = true;
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    });
    if (route === "reopen") {
      view.unmount();
      view = render(
        createElement(SourceAvailability, { account: "alice", track }),
      );
      await act(async () => {
        fireEvent.click(
          screen.getByRole("button", { name: "Check availability" }),
        );
      });
    }
    expect(screen.getByText("No audio is ready yet.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Play track" })).toBeNull();
    expect(requests).toHaveLength(1);
    const prepare = screen.getByRole("button", { name: "Prepare audio" });
    expect(prepare).toHaveProperty("disabled", false);
    await act(async () => {
      fireEvent.click(prepare);
      fireEvent.click(prepare);
    });
    expect(requests).toHaveLength(2);
    expect(requests[1]).not.toBe(requests[0]);
    expect(requests).toEqual([
      expect.stringMatching(
        /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
      ),
      expect.stringMatching(
        /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
      ),
    ]);
    expect(screen.getByText("Preparing audio…")).toBeTruthy();
    expect(
      screen.queryByText(
        "This source has a different preparation. Check audio availability again.",
      ),
    ).toBeNull();
    expect(player.playQueue).not.toHaveBeenCalled();
    expect(
      fetch.mock.calls.every(
        ([url]) => !url.includes("/download") && !url.includes("/audio"),
      ),
    ).toBe(true);
    view.unmount();
    expect(vi.getTimerCount()).toBe(0);
  },
);

test("readiness for another recording is rejected and cannot publish a playable snapshot", async () => {
  vi.useFakeTimers();
  const fetch = vi.fn(async (url: string) => {
    if (url.endsWith("/refresh")) return Response.json([source]);
    if (url === preparationPath) return Response.json(status("ready"));
    if (url === sourcesPath)
      return Response.json([
        { ...source, recording_id: preparation, ready: true },
      ]);
    throw new Error("Unexpected preparation request");
  });
  vi.stubGlobal("fetch", fetch);
  render(createElement(SourceAvailability, { account: "alice", track }));
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
  });
  expect(
    screen.getByText(
      "Hito could not confirm playable audio. Check availability again.",
    ),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Play track" })).toBeNull();
  expect(player.playQueue).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});

test("only capability-eligible sources expose preparation and selecting another source aborts its one observer", async () => {
  const second = { ...source, id: preparation, label: "Other owned audio" };
  const pending = heldResponse();
  const fetch = vi.fn(async (url: string, _options?: RequestInit) => {
    if (url.endsWith("/refresh"))
      return Response.json([
        {
          ...source,
          id: "local-source",
          label: "Local audio",
          can_prepare: false,
          provider: "local",
        },
        source,
        second,
      ]);
    if (url === preparationPath) return pending.response;
    if (url === `${sourcesPath}/${preparation}/preparation`)
      return new Response(null, { status: 404 });
    throw new Error("Unexpected preparation request");
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(SourceAvailability, { account: "alice", track }),
  );
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
  });
  expect(screen.queryByRole("radio", { name: "Local audio" })).toBeNull();
  expect(screen.getByRole("radio", { name: source.label })).toHaveProperty(
    "checked",
    true,
  );
  const signal = fetch.mock.calls[1][1]?.signal;
  await act(async () => {
    fireEvent.click(screen.getByRole("radio", { name: second.label }));
  });
  expect(signal?.aborted).toBe(true);
  expect(screen.getByRole("radio", { name: second.label })).toHaveProperty(
    "checked",
    true,
  );
  expect(screen.getByRole("button", { name: "Prepare audio" })).toBeTruthy();
  await act(async () => {
    pending.resolve(Response.json(status("ready")));
  });
  expect(screen.queryByRole("button", { name: "Play track" })).toBeNull();
  expect(fetch.mock.calls.map(([url]) => url)).toEqual([
    `${sourcesPath}/refresh`,
    preparationPath,
    `${sourcesPath}/${preparation}/preparation`,
  ]);
  expect(player.playQueue).not.toHaveBeenCalled();
  view.unmount();
});

test("a known failed preparation permits a fresh explicit request without resubmitting automatically", async () => {
  vi.useFakeTimers();
  const replacement = "b9fa456e-cc9f-440a-9ebc-305aac9943b3";
  let writes = 0;
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    if (url.endsWith("/refresh")) return Response.json([source]);
    if (url === preparationPath && options?.method === "POST") {
      writes++;
      return Response.json(
        {
          request_id: JSON.parse(String(options.body)).request_id,
          preparation_id: replacement,
          recording_id: recording,
          source_id: sourceId,
        },
        { status: 202 },
      );
    }
    if (url === preparationPath)
      return Response.json(
        writes
          ? { ...status("preparing"), preparation_id: replacement }
          : status("failed"),
      );
    throw new Error("Unexpected failed preparation request");
  });
  vi.stubGlobal("fetch", fetch);
  render(createElement(SourceAvailability, { account: "alice", track }));
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
  });
  expect(screen.getByText("Audio preparation failed.")).toBeTruthy();
  expect(writes).toBe(0);
  const prepare = screen.getByRole("button", { name: "Prepare audio" });
  await act(async () => {
    fireEvent.click(prepare);
    fireEvent.click(prepare);
  });
  expect(writes).toBe(1);
  expect(screen.getByText("Preparing audio…")).toBeTruthy();
  expect(
    screen.queryByText(
      "This source has a different preparation. Check audio availability again.",
    ),
  ).toBeNull();
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("an explicit same-source availability refresh recovers a halted latest-identity observation", async () => {
  vi.useFakeTimers();
  const replacement = "bef7838d-dd36-4254-8148-29374e374e17";
  let checks = 0;
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    if (url.endsWith("/refresh")) return Response.json([source]);
    if (url === preparationPath && options?.method === "GET") {
      checks++;
      return Response.json({
        ...status("preparing"),
        preparation_id: checks === 1 ? preparation : replacement,
      });
    }
    throw new Error("Unexpected halted preparation request");
  });
  vi.stubGlobal("fetch", fetch);
  render(createElement(SourceAvailability, { account: "alice", track }));
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5_000);
  });
  expect(
    screen.getByText(
      "This source has a different preparation. Check audio availability again.",
    ),
  ).toBeTruthy();
  expect(vi.getTimerCount()).toBe(0);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
  });
  expect(screen.getByText("Preparing audio…")).toBeTruthy();
  expect(
    screen.queryByText(
      "This source has a different preparation. Check audio availability again.",
    ),
  ).toBeNull();
  expect(checks).toBe(3);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method === "POST")
      .every(([url]) => url.endsWith("/sources/refresh")),
  ).toBe(true);
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("a successful same-source availability check preserves an unacknowledged POST instead of adopting latest status", async () => {
  vi.useFakeTimers();
  const bodies: string[] = [];
  let reads = 0;
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    if (url.endsWith("/refresh")) return Response.json([source]);
    if (url === preparationPath && options?.method === "GET") {
      reads++;
      return bodies.length
        ? Response.json(status("preparing"))
        : new Response(null, { status: 404 });
    }
    if (url === preparationPath && options?.method === "POST") {
      bodies.push(String(options.body));
      if (bodies.length === 1)
        throw new TypeError("private lost request acknowledgement");
      return Response.json(
        {
          request_id: JSON.parse(bodies[0]).request_id,
          preparation_id: preparation,
          recording_id: recording,
          source_id: sourceId,
        },
        { status: 202 },
      );
    }
    throw new Error("Unexpected pending preparation request");
  });
  vi.stubGlobal("fetch", fetch);
  render(createElement(SourceAvailability, { account: "alice", track }));
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
  });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Prepare audio" }));
  });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
  });
  expect(
    screen.getByRole("button", { name: "Retry preparation request" }),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Prepare audio" })).toBeNull();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(reads).toBe(1);
  expect(bodies).toHaveLength(1);
  await act(async () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Retry preparation request" }),
    );
  });
  expect(bodies).toEqual([bodies[0], bodies[0]]);
  expect(reads).toBe(2);
  expect(screen.getByText("Preparing audio…")).toBeTruthy();
  expect(player.playQueue).not.toHaveBeenCalled();
});

test("a late ready confirmation cannot overwrite a newer successful unavailable snapshot", async () => {
  vi.useFakeTimers();
  const confirmation = heldResponse();
  let sourceReads = 0;
  const fetch = vi.fn(async (url: string) => {
    if (url.endsWith("/refresh")) return Response.json([source]);
    if (url === preparationPath) return Response.json(status("ready"));
    if (url === sourcesPath) {
      sourceReads++;
      return confirmation.response;
    }
    throw new Error("Unexpected readiness race request");
  });
  vi.stubGlobal("fetch", fetch);
  render(createElement(SourceAvailability, { account: "alice", track }));
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
  });
  expect(sourceReads).toBe(1);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
  });
  await act(async () => {
    confirmation.resolve(
      Response.json([{ ...source, ready: true, can_prepare: false }]),
    );
  });
  expect(screen.getByText("No audio is ready yet.")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Play track" })).toBeNull();
  expect(
    screen.getByText(
      "Hito could not confirm playable audio. Check availability again.",
    ),
  ).toBeTruthy();
  expect(sourceReads).toBe(1);
  expect(player.playQueue).not.toHaveBeenCalled();
});
