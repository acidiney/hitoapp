import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";

function reset() {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  localStorage.clear();
  sessionStorage.clear();
}

beforeEach(reset);
afterEach(reset);
