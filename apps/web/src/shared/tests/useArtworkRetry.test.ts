import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useArtworkRetry } from "../hooks/useArtworkRetry";

const DELAYS = [100, 300];

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

function mount(source = "a") {
  return renderHook(({ source }) => useArtworkRetry(DELAYS, source), {
    initialProps: { source },
  });
}

test("a miss waits for the exact delay and then shows the next attempt", () => {
  const view = mount();
  expect(view.result.current).toMatchObject({ attempt: 0, waiting: false });
  act(() => view.result.current.miss());
  expect(view.result.current).toMatchObject({ attempt: 0, waiting: true });
  act(() => vi.advanceTimersByTime(99));
  expect(view.result.current.waiting).toBe(true);
  act(() => vi.advanceTimersByTime(1));
  expect(view.result.current).toMatchObject({ attempt: 1, waiting: false });
  act(() => view.result.current.miss());
  act(() => vi.advanceTimersByTime(299));
  expect(view.result.current.waiting).toBe(true);
  act(() => vi.advanceTimersByTime(1));
  expect(view.result.current).toMatchObject({ attempt: 2, waiting: false });
});

test("it stops after the last delay", () => {
  const view = mount();
  for (const delay of DELAYS) {
    act(() => view.result.current.miss());
    act(() => vi.advanceTimersByTime(delay));
  }
  expect(view.result.current.exhausted).toBe(false);
  act(() => view.result.current.miss());
  expect(view.result.current).toMatchObject({
    exhausted: true,
    waiting: false,
  });
  expect(vi.getTimerCount()).toBe(0);
});

test("a new source starts again from the first attempt", () => {
  const view = mount("a");
  act(() => view.result.current.miss());
  act(() => vi.advanceTimersByTime(100));
  expect(view.result.current.attempt).toBe(1);
  view.rerender({ source: "b" });
  expect(view.result.current).toMatchObject({
    attempt: 0,
    waiting: false,
    exhausted: false,
  });
  act(() => view.result.current.miss());
  act(() => vi.advanceTimersByTime(100));
  expect(view.result.current.attempt).toBe(1);
});

test("a new source cancels a pending retry", () => {
  const view = mount("a");
  act(() => view.result.current.miss());
  view.rerender({ source: "b" });
  expect(vi.getTimerCount()).toBe(0);
  act(() => vi.advanceTimersByTime(1000));
  expect(view.result.current).toMatchObject({ attempt: 0, waiting: false });
});

test("unmounting clears the pending timer", () => {
  const view = mount();
  act(() => view.result.current.miss());
  expect(vi.getTimerCount()).toBe(1);
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
});
