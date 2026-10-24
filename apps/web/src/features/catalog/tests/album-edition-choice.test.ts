import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useAlbumEditionChoice } from "../hooks/useAlbumEditionChoice";
import {
  albumDetail,
  albumId,
  choiceReceipt,
  readyChoice,
  savedEdition,
} from "../fixtures/albumEditions";

const choose = `/api/v1/albums/${albumId}/edition`;
const clear = `/api/v1/albums/${albumId}/edition/clear`;
const current = `/api/v1/releases/${albumId}?limit=1`;
const other = { ...savedEdition, edition_id: "opaque:edition/other" };
const unset = { revision: 3, state: { status: "unset" } } as const;

type Route = (init?: RequestInit) => Response | Promise<Response>;

function serve(routes: Record<string, Route>) {
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    const route = routes[path];
    if (!route) throw new Error(`Unexpected request ${path}`);
    return route(init);
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

const bodies = (fetch: ReturnType<typeof serve>, path: string) =>
  fetch.mock.calls
    .filter(([called]) => called === path)
    .map(([, init]) => JSON.parse(init!.body as string));

afterEach(() => vi.unstubAllGlobals());

test("the newest choice is saved after the one in flight and only it is reported as remembered", async () => {
  let release!: (response: Response) => void;
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  let calls = 0;
  const fetch = serve({
    [choose]: (init) => {
      calls += 1;
      if (calls === 1) return held;
      const body = JSON.parse(init!.body as string);
      return Response.json({
        ...choiceReceipt(5, body),
        request_id: body.request_id,
      });
    },
  });
  const { result } = renderHook(() =>
    useAlbumEditionChoice("account", albumId, unset),
  );
  let first!: Promise<boolean>;
  act(() => {
    first = result.current.choose(savedEdition);
  });
  await waitFor(() => expect(calls).toBe(1));
  act(() => {
    void result.current.choose(other);
  });
  expect(result.current.remembered).toBeNull();
  await act(async () => {
    release(Response.json(choiceReceipt(4)));
    await first;
  });
  await waitFor(() => expect(result.current.remembered).toEqual(other));
  const sent = bodies(fetch, choose);
  expect(sent).toHaveLength(2);
  expect(sent[0]).toMatchObject({ expected_revision: 3, ...savedEdition });
  expect(sent[1]).toMatchObject({ expected_revision: 4, ...other });
  expect(result.current.choice.revision).toBe(5);
});

test("reading another edition forgets the remembered notice", async () => {
  serve({ [choose]: () => Response.json(choiceReceipt(4)) });
  const { result } = renderHook(() =>
    useAlbumEditionChoice("account", albumId, unset),
  );
  await act(async () => {
    await result.current.choose(savedEdition);
  });
  expect(result.current.remembered).toEqual(savedEdition);
  act(() => result.current.reading());
  expect(result.current.remembered).toBeNull();
});

test("a lost acknowledgement is retried with the same request and body", async () => {
  let calls = 0;
  const fetch = serve({
    [choose]: (init) => {
      calls += 1;
      if (calls === 1) throw new TypeError("network down");
      const body = JSON.parse(init!.body as string);
      return Response.json({
        ...choiceReceipt(4),
        request_id: body.request_id,
      });
    },
    [clear]: (init) =>
      Response.json({
        ...choiceReceipt(5),
        edition: null,
        request_id: JSON.parse(init!.body as string).request_id,
      }),
  });
  const { result } = renderHook(() =>
    useAlbumEditionChoice("account", albumId, unset),
  );
  await act(async () => {
    expect(await result.current.choose(savedEdition)).toBe(false);
  });
  expect(result.current.error).not.toBe("");
  await act(async () => {
    expect(await result.current.retry()).toBe(true);
  });
  const sent = bodies(fetch, choose);
  expect(sent).toHaveLength(2);
  expect(sent[1]).toEqual(sent[0]);
  expect(result.current.remembered).toEqual(savedEdition);
  await act(async () => {
    await result.current.forget();
  });
  expect(bodies(fetch, clear)[0].expected_revision).toBe(4);
});

test("a conflict reloads the saved edition and revision and offers the action again", async () => {
  let calls = 0;
  const fetch = serve({
    [choose]: (init) => {
      calls += 1;
      if (calls === 1)
        return Response.json({ error: "Changed" }, { status: 409 });
      const body = JSON.parse(init!.body as string);
      return Response.json({
        ...choiceReceipt(10, body),
        request_id: body.request_id,
      });
    },
    [current]: () =>
      Response.json({
        ...albumDetail,
        saved_edition: { ...readyChoice, revision: 9 },
      }),
  });
  const { result } = renderHook(() =>
    useAlbumEditionChoice("account", albumId, unset),
  );
  await act(async () => {
    expect(await result.current.choose(other)).toBe(false);
  });
  expect(result.current.choice.revision).toBe(9);
  expect(result.current.error).toMatch(/changed/i);
  await act(async () => {
    expect(await result.current.retry()).toBe(true);
  });
  const sent = bodies(fetch, choose);
  expect(sent[1].expected_revision).toBe(9);
  expect(sent[1].request_id).not.toBe(sent[0].request_id);
  expect(result.current.remembered).toEqual(other);
});
