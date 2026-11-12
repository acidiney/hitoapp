import { act, render } from "@testing-library/react";
import { createElement } from "react";
import { expect, test, vi } from "vitest";
import Skeleton from "../ui/Skeleton";

test("skeleton rows appear only after 300 ms and are hidden from assistive technology", () => {
  vi.useFakeTimers();
  const view = render(createElement(Skeleton, { rows: 3 }));
  expect(view.container.querySelector(".skeleton")).toBeNull();
  act(() => {
    vi.advanceTimersByTime(299);
  });
  expect(view.container.querySelector(".skeleton")).toBeNull();
  act(() => {
    vi.advanceTimersByTime(1);
  });
  const rows = view.container.querySelector(".skeleton");
  expect(rows?.getAttribute("aria-hidden")).toBe("true");
  expect(rows?.querySelectorAll(".skeleton-row")).toHaveLength(3);
});
