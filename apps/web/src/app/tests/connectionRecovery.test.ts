import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import ConnectionErrorPage from "../ConnectionErrorPage";
import {
  installConnectionRecovery,
  type RecoveryRouter,
} from "../connectionRecovery";
import {
  getConnection,
  reportReachable,
  reportUnreachable,
  resetConnection,
} from "../../shared/connection";

type Handler = (event: { preventDefault: () => void; detail: unknown }) => void;

function fakeRouter() {
  const handlers = new Map<string, Handler>();
  const router: RecoveryRouter = {
    on: (name, handler) => {
      handlers.set(name, handler as Handler);
      return () => handlers.delete(name);
    },
    reload: vi.fn(),
  };
  const fire = (name: string, detail: unknown) => {
    const event = { preventDefault: vi.fn(), detail };
    handlers.get(name)?.(event);
    return event;
  };
  return { router, fire, handlers };
}

beforeEach(() => {
  vi.useFakeTimers();
  resetConnection();
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("{}")));
});
afterEach(() => resetConnection());

test("a failed page visit is an outage and its raw error dialog is suppressed", () => {
  const { router, fire } = fakeRouter();
  installConnectionRecovery(router);
  const event = fire("networkError", { error: new Error("Network Error") });
  expect(event.preventDefault).toHaveBeenCalled();
  expect(getConnection()).toMatchObject({
    reachable: false,
    reason: "network",
  });
});

test("a proxy error page is an outage and not shown to the user", () => {
  const { router, fire } = fakeRouter();
  installConnectionRecovery(router);
  const event = fire("httpException", { response: { status: 502 } });
  expect(event.preventDefault).toHaveBeenCalled();
  expect(getConnection()).toMatchObject({
    reachable: false,
    reason: "gateway",
  });
});

test("another non-Inertia status is left to the default handling", () => {
  const { router, fire } = fakeRouter();
  installConnectionRecovery(router);
  const event = fire("httpException", { response: { status: 500 } });
  expect(event.preventDefault).not.toHaveBeenCalled();
  expect(getConnection().reachable).toBe(true);
});

test("when the server returns after a failed visit the current page reloads in place", async () => {
  const { router, fire } = fakeRouter();
  installConnectionRecovery(router);
  fire("networkError", { error: new Error("x") });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2_000);
  });
  expect(router.reload).toHaveBeenCalledTimes(1);
});

test("an outage that only hit background requests does not reload the page", () => {
  const { router } = fakeRouter();
  installConnectionRecovery(router);
  reportUnreachable("network");
  reportReachable();
  expect(router.reload).not.toHaveBeenCalled();
});

test("disposing stops listening", () => {
  const { router, handlers } = fakeRouter();
  installConnectionRecovery(router)();
  expect(handlers.size).toBe(0);
});

test("the start-up error page reloads by itself when the server comes back", async () => {
  const reload = vi.fn();
  vi.stubGlobal("location", { ...window.location, reload });
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("x")));
  render(createElement(ConnectionErrorPage));
  expect(
    screen.getByRole("heading", { name: "Cannot reach Hito" }),
  ).toBeTruthy();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2_000);
  });
  expect(reload).not.toHaveBeenCalled();
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("{}")));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(4_000);
  });
  expect(reload).toHaveBeenCalledTimes(1);
});

test("the start-up error page does not reload in a loop when the server answers but the session still fails", async () => {
  const reload = vi.fn();
  vi.stubGlobal("location", { ...window.location, reload });
  sessionStorage.setItem("hito-connection-reload", String(Date.now()));
  render(createElement(ConnectionErrorPage));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2_000);
  });
  expect(reload).not.toHaveBeenCalled();
});

test("Try now on the start-up error page reloads at once", () => {
  const reload = vi.fn();
  vi.stubGlobal("location", { ...window.location, reload });
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("x")));
  render(createElement(ConnectionErrorPage));
  fireEvent.click(screen.getByRole("button", { name: "Try now" }));
  expect(reload).toHaveBeenCalledTimes(1);
});
