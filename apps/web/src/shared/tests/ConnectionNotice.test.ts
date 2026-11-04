import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import ConnectionNotice from "../ui/ConnectionNotice";
import {
  markChangeLost,
  reportReachable,
  reportUnreachable,
  resetConnection,
} from "../connection";

beforeEach(() => {
  vi.useFakeTimers();
  resetConnection();
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("x")));
});
afterEach(() => resetConnection());

test("nothing shows while the server is reachable", () => {
  const { container } = render(createElement(ConnectionNotice));
  expect(container.textContent).toBe("");
});

test.each([
  ["network", /Check your internet connection and that Hito is running/],
  ["timeout", /Hito is taking too long to answer/],
  ["gateway", /Hito is not responding right now/],
] as const)("a %s outage explains itself in plain words", (reason, detail) => {
  render(createElement(ConnectionNotice));
  act(() => reportUnreachable(reason));
  const notice = screen.getByRole("status");
  expect(notice.textContent).toContain("Cannot reach Hito");
  expect(notice.textContent).toMatch(detail);
  expect(notice.textContent).toContain("Reconnecting automatically");
  expect(notice.textContent).not.toMatch(/\b50\d\b|fetch|http|\//i);
});

test("the notice disappears by itself when the server returns", () => {
  render(createElement(ConnectionNotice));
  act(() => reportUnreachable("network"));
  expect(screen.getByRole("status")).toBeTruthy();
  act(() => reportReachable());
  expect(screen.queryByRole("status")).toBeNull();
});

test("Try now probes the server straight away", async () => {
  const fetchMock = vi.fn().mockResolvedValue(new Response("{}"));
  vi.stubGlobal("fetch", fetchMock);
  render(createElement(ConnectionNotice));
  act(() => reportUnreachable("network"));
  fireEvent.click(screen.getByRole("button", { name: "Try now" }));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(fetchMock).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("status")).toBeNull();
});

test("a lost change is told plainly, outlives the outage and can be dismissed", () => {
  render(createElement(ConnectionNotice));
  act(() => {
    reportUnreachable("network");
    markChangeLost();
  });
  const alert = screen.getByRole("alert");
  expect(alert.textContent).toContain("Your last change was not saved");
  act(() => reportReachable());
  expect(screen.queryByRole("status")).toBeNull();
  expect(screen.getByRole("alert")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Dismiss message" }));
  expect(screen.queryByRole("alert")).toBeNull();
});
