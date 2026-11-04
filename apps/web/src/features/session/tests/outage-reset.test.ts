import { expect, test, vi } from "vitest";
import { notifySessionChanged } from "../api/session";
import {
  getConnection,
  getLostChange,
  markChangeLost,
  reportUnreachable,
} from "../../../shared/connection";

test("signing out or switching account clears an outage and a lost change", () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("x")));
  reportUnreachable("network");
  markChangeLost();
  notifySessionChanged();
  expect(getConnection().reachable).toBe(true);
  expect(getLostChange()).toBe(false);
});
