import { afterEach, expect, test, vi } from "vitest";
import { focusPageHeading } from "../ui/RouteLink";

afterEach(() => {
  document.body.innerHTML = "";
  vi.useRealTimers();
});

test("the first page heading takes focus after navigation", () => {
  document.body.innerHTML = '<main><h1 id="top">Queue</h1></main>';
  vi.useFakeTimers();
  focusPageHeading();
  vi.runAllTimers();
  expect(document.activeElement?.id).toBe("top");
});
