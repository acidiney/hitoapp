import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";
import { resetConnection } from "../shared/connection";
import { catalogCacheClock } from "../features/artists/api/catalogCache";

HTMLDialogElement.prototype.showModal = function () {
  this.setAttribute("open", "");
};
HTMLDialogElement.prototype.close = function () {
  this.removeAttribute("open");
};

function reset() {
  cleanup();
  resetConnection();
  catalogCacheClock.reset();
  catalogCacheClock.setRetryDelay(0);
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  localStorage.clear();
  sessionStorage.clear();
}

beforeEach(reset);
afterEach(reset);
