import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import {
  FRESH_MS,
  STALE_MS,
  cachedRead,
  catalogCacheClock,
  forgetCatalog,
  rememberReleaseGroups,
  rememberedReleaseGroup,
  useCatalogRefreshing,
} from "../api/catalogCache";

const signal = new AbortController().signal;
const available = (value: string) => ({ status: "available", value });

function clock() {
  let now = 1_000;
  catalogCacheClock.set(() => now);
  return (ms: number) => {
    now += ms;
  };
}

test("a fresh answer is served without asking the provider again", async () => {
  clock();
  const load = vi.fn(async () => available("one"));
  await cachedRead("a", ["x"], load, signal);
  const again = await cachedRead("a", ["x"], load, signal);
  expect(again.value).toBe("one");
  expect(load).toHaveBeenCalledTimes(1);
});

test("accounts and request bodies never share an answer", async () => {
  clock();
  const load = vi.fn(async () => available("one"));
  await cachedRead("a", ["x"], load, signal);
  await cachedRead("b", ["x"], load, signal);
  await cachedRead("a", ["y"], load, signal);
  expect(load).toHaveBeenCalledTimes(3);
});

test("unavailable outcomes are not remembered", async () => {
  clock();
  const load = vi
    .fn<() => Promise<{ status: string; value?: string }>>()
    .mockResolvedValueOnce({ status: "unavailable" })
    .mockResolvedValueOnce(available("two"));
  expect((await cachedRead("a", ["x"], load, signal)).status).toBe(
    "unavailable",
  );
  expect((await cachedRead("a", ["x"], load, signal)).value).toBe("two");
});

test("a stale answer shows at once and refreshes quietly in the background", async () => {
  const advance = clock();
  const gate: { release: (value: unknown) => void } = {
    release: () => undefined,
  };
  const load = vi
    .fn<() => Promise<{ status: string; value: string }>>()
    .mockResolvedValueOnce(available("old"))
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          gate.release = resolve as (value: unknown) => void;
        }),
    );
  await cachedRead("a", ["x"], load, signal);
  advance(FRESH_MS + 1);
  const view = renderHook(() => useCatalogRefreshing());
  expect(view.result.current).toBe(false);
  const shown = await cachedRead("a", ["x"], load, signal);
  expect(shown.value).toBe("old");
  await waitFor(() => expect(view.result.current).toBe(true));
  await act(async () => gate.release(available("new")));
  await waitFor(() => expect(view.result.current).toBe(false));
  expect((await cachedRead("a", ["x"], load, signal)).value).toBe("new");
  expect(load).toHaveBeenCalledTimes(2);
});

test("an answer older than the stale limit blocks on a fresh read", async () => {
  const advance = clock();
  const load = vi
    .fn<() => Promise<{ status: string; value: string }>>()
    .mockResolvedValueOnce(available("old"))
    .mockResolvedValueOnce(available("new"));
  await cachedRead("a", ["x"], load, signal);
  advance(STALE_MS + 1);
  expect((await cachedRead("a", ["x"], load, signal)).value).toBe("new");
});

test("forgetting one account keeps the others", async () => {
  clock();
  const load = vi.fn(async () => available("one"));
  await cachedRead("a", ["x"], load, signal);
  await cachedRead("b", ["x"], load, signal);
  rememberReleaseGroups("a", "assoc", [
    { id: "g", title: "T", secondary_types: [] },
  ]);
  forgetCatalog("a");
  expect(rememberedReleaseGroup("a", "assoc", "g")).toBeNull();
  await cachedRead("b", ["x"], load, signal);
  expect(load).toHaveBeenCalledTimes(2);
});
