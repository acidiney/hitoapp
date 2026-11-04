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
  getLostChange,
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
  const event = fire("httpException", {
    response: { status: 502, data: "<html>Bad Gateway</html>" },
  });
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

test.each([
  ["a parsed Hito error", { error: "The provider is unavailable." }],
  [
    "a Hito error left as JSON text",
    '{"error":"The provider is unavailable."}',
  ],
])("a 5xx page visit carrying %s keeps its own handling", (_name, data) => {
  const { router, fire } = fakeRouter();
  installConnectionRecovery(router);
  for (const status of [502, 503, 504]) {
    const event = fire("httpException", { response: { status, data } });
    expect(event.preventDefault).not.toHaveBeenCalled();
  }
  expect(getConnection().reachable).toBe(true);
});

test("a gateway visit with an empty or object body without an error string is an outage", () => {
  const { router, fire } = fakeRouter();
  installConnectionRecovery(router);
  const event = fire("httpException", {
    response: { status: 503, data: { message: "x" } },
  });
  expect(event.preventDefault).toHaveBeenCalled();
  expect(getConnection().reachable).toBe(false);
});

test("a route that keeps failing while health is fine reloads at most once per window", async () => {
  const { router, fire } = fakeRouter();
  installConnectionRecovery(router);
  const failAndRecover = async () => {
    fire("httpException", { response: { status: 502, data: "x" } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_000);
    });
  };
  await failAndRecover();
  await failAndRecover();
  await failAndRecover();
  expect(router.reload).toHaveBeenCalledTimes(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(15_000);
  });
  await failAndRecover();
  expect(router.reload).toHaveBeenCalledTimes(2);
});

test("a failed form submission is reported as unsaved and never resubmitted", async () => {
  const { router, fire } = fakeRouter();
  installConnectionRecovery(router);
  fire("before", { visit: { method: "post" } });
  fire("networkError", { error: new Error("x") });
  expect(getLostChange()).toBe(true);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2_000);
  });
  expect(getLostChange()).toBe(true);
  expect(router).not.toHaveProperty("visit");
});

test("a gateway page for a non-GET visit is reported as unsaved", () => {
  const { router, fire } = fakeRouter();
  installConnectionRecovery(router);
  fire("before", { visit: { method: "delete" } });
  fire("httpException", { response: { status: 502, data: "x" } });
  expect(getLostChange()).toBe(true);
});

test("a failed GET visit is not reported as an unsaved change", () => {
  const { router, fire } = fakeRouter();
  installConnectionRecovery(router);
  fire("before", { visit: { method: "get" } });
  fire("networkError", { error: new Error("x") });
  expect(getLostChange()).toBe(false);
});

test("a reload skipped by the limiter is retried after the remaining wait", async () => {
  const reload = vi.fn();
  vi.stubGlobal("location", { ...window.location, reload });
  sessionStorage.setItem("hito-connection-reload", String(Date.now() - 10_000));
  render(createElement(ConnectionErrorPage));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2_000);
  });
  expect(reload).not.toHaveBeenCalled();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(4_000);
  });
  expect(reload).toHaveBeenCalledTimes(1);
});

test("leaving the start-up error page cancels a pending retry", async () => {
  const reload = vi.fn();
  vi.stubGlobal("location", { ...window.location, reload });
  sessionStorage.setItem("hito-connection-reload", String(Date.now() - 10_000));
  const view = render(createElement(ConnectionErrorPage));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2_000);
  });
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(reload).not.toHaveBeenCalled();
});
