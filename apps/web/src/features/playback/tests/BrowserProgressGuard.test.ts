import { describe, expect, test, vi } from "vitest";
import { BrowserProgressGuard } from "../browser/BrowserProgressGuard";

const MESSAGE = "Listening progress is waiting to be saved. Playback paused.";

function guarded(options: { admits?: boolean; paused?: boolean } = {}) {
  const state = {
    admits: options.admits ?? true,
    paused: options.paused ?? false,
  };
  const observation = {
    persist: vi.fn(() => state.admits),
    observe: vi.fn(() => state.admits),
    retry: vi.fn(() => state.admits),
  };
  const pause = vi.fn(() => {
    state.paused = true;
  });
  const fail = vi.fn();
  const guard = new BrowserProgressGuard(
    observation,
    { isPaused: () => state.paused, pause },
    fail,
  );
  return { guard, observation, pause, fail, state };
}

describe("persist", () => {
  test("returns true without side effects when the checkpoint is admitted", () => {
    const { guard, pause, fail } = guarded();
    expect(guard.persist()).toBe(true);
    expect(pause).not.toHaveBeenCalled();
    expect(fail).not.toHaveBeenCalled();
  });
  test("pauses and reports once when admission is refused", () => {
    const { guard, pause, fail } = guarded({ admits: false });
    expect(guard.persist()).toBe(false);
    expect(pause).toHaveBeenCalledTimes(1);
    expect(fail).toHaveBeenCalledWith(MESSAGE);
  });
  test("does not pause media that is already paused but still reports", () => {
    const { guard, pause, fail } = guarded({ admits: false, paused: true });
    guard.persist();
    expect(pause).not.toHaveBeenCalled();
    expect(fail).toHaveBeenCalledWith(MESSAGE);
  });
});

describe("observe", () => {
  test("blocks playback when an observed sample is refused", () => {
    const { guard, pause, fail } = guarded({ admits: false });
    guard.observe();
    expect(pause).toHaveBeenCalledTimes(1);
    expect(fail).toHaveBeenCalledWith(MESSAGE);
  });
  test("is silent while samples are admitted", () => {
    const { guard, pause, fail } = guarded();
    guard.observe();
    expect(pause).not.toHaveBeenCalled();
    expect(fail).not.toHaveBeenCalled();
  });
});

describe("blocking is not re-entrant", () => {
  test("a pause that triggers another persist does not pause or report twice", () => {
    const { guard, observation, pause, fail } = guarded({ admits: false });
    pause.mockImplementation(() => {
      guard.persist();
    });
    guard.persist();
    expect(pause).toHaveBeenCalledTimes(1);
    expect(fail).toHaveBeenCalledTimes(1);
    expect(observation.persist).toHaveBeenCalledTimes(2);
  });
  test("a later refusal blocks again once the first finished", () => {
    const { guard, fail, state } = guarded({ admits: false });
    guard.persist();
    state.paused = false;
    guard.persist();
    expect(fail).toHaveBeenCalledTimes(2);
  });
  test("exposes whether the pause came from progress", () => {
    const { guard, pause } = guarded({ admits: false });
    let during = false;
    pause.mockImplementation(() => {
      during = guard.stopping;
    });
    guard.persist();
    expect(during).toBe(true);
    expect(guard.stopping).toBe(false);
  });
});

describe("retry", () => {
  test("reports whether the pending checkpoint was admitted", () => {
    const { guard, observation, state } = guarded({ admits: false });
    expect(guard.retry()).toBe(false);
    state.admits = true;
    expect(guard.retry()).toBe(true);
    expect(observation.retry).toHaveBeenCalledTimes(2);
  });
});
