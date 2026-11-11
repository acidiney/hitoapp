import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import {
  canRegisterServiceWorker,
  registerServiceWorker,
  type ServiceWorkerEnvironment,
} from "../serviceWorker";
import UpdateNotice from "../UpdateNotice";

beforeEach(() => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response(null, { status: 200 })),
  );
});

type Listener = () => void;

function fakeContainer(options: { controller: boolean; waiting?: boolean }) {
  const listeners = new Map<string, Listener[]>();
  const waiting = { postMessage: vi.fn() };
  const registration = {
    waiting: options.waiting ? waiting : null,
    installing: null as unknown,
    listeners: new Map<string, Listener[]>(),
    addEventListener(name: string, listener: Listener) {
      registration.listeners.set(name, [
        ...(registration.listeners.get(name) ?? []),
        listener,
      ]);
    },
  };
  const container = {
    controller: options.controller ? {} : null,
    register: vi.fn().mockResolvedValue(registration),
    addEventListener(name: string, listener: Listener) {
      listeners.set(name, [...(listeners.get(name) ?? []), listener]);
    },
  };
  return {
    container: container as unknown as ServiceWorkerContainer,
    register: container.register,
    registration,
    waiting,
    fire: (name: string) => listeners.get(name)?.forEach((run) => run()),
  };
}

function environment(
  overrides: Partial<ServiceWorkerEnvironment>,
): ServiceWorkerEnvironment {
  return {
    production: true,
    secureContext: true,
    container: fakeContainer({ controller: true }).container,
    ...overrides,
  };
}

test("registration needs a production build, a secure context and service worker support", () => {
  expect(canRegisterServiceWorker(environment({}))).toBe(true);
  expect(canRegisterServiceWorker(environment({ production: false }))).toBe(
    false,
  );
  expect(canRegisterServiceWorker(environment({ secureContext: false }))).toBe(
    false,
  );
  expect(canRegisterServiceWorker(environment({ container: undefined }))).toBe(
    false,
  );
});

test("under Vitest the current environment is not a production build", async () => {
  const { currentEnvironment } = await import("../serviceWorker");
  expect(canRegisterServiceWorker(currentEnvironment())).toBe(false);
});

test("a blocked environment never calls register", async () => {
  const fake = fakeContainer({ controller: true });
  const registered = await registerServiceWorker(
    environment({ production: false, container: fake.container }),
    vi.fn(),
  );
  expect(registered).toBe(false);
  expect(fake.register).not.toHaveBeenCalled();
});

test("a server that does not serve the worker script is left unregistered", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response(null, { status: 404 })),
  );
  const fake = fakeContainer({ controller: true });
  const registered = await registerServiceWorker(
    environment({ container: fake.container }),
    vi.fn(),
  );
  expect(registered).toBe(false);
  expect(fake.register).not.toHaveBeenCalled();
});

test("the worker registers from the root with scope /", async () => {
  const fake = fakeContainer({ controller: true });
  const registered = await registerServiceWorker(
    environment({ container: fake.container }),
    vi.fn(),
  );
  expect(registered).toBe(true);
  expect(fake.register).toHaveBeenCalledWith("/sw.js", { scope: "/" });
});

test("a waiting worker next to a controlled page offers the update, and applying it reloads once", async () => {
  const fake = fakeContainer({ controller: true, waiting: true });
  const reload = vi.fn();
  const onUpdateReady = vi.fn();
  await registerServiceWorker(
    environment({ container: fake.container }),
    onUpdateReady,
    reload,
  );
  expect(onUpdateReady).toHaveBeenCalledTimes(1);
  onUpdateReady.mock.calls[0][0]();
  expect(fake.waiting.postMessage).toHaveBeenCalledWith({
    type: "SKIP_WAITING",
  });
  expect(reload).not.toHaveBeenCalled();
  fake.fire("controllerchange");
  fake.fire("controllerchange");
  expect(reload).toHaveBeenCalledTimes(1);
});

test("the first install on an uncontrolled page offers no update", async () => {
  const fake = fakeContainer({ controller: false, waiting: true });
  const onUpdateReady = vi.fn();
  await registerServiceWorker(
    environment({ container: fake.container }),
    onUpdateReady,
  );
  expect(onUpdateReady).not.toHaveBeenCalled();
});

test("a worker that finishes installing later offers the update", async () => {
  const fake = fakeContainer({ controller: true });
  const onUpdateReady = vi.fn();
  await registerServiceWorker(
    environment({ container: fake.container }),
    onUpdateReady,
  );
  const statechange: Listener[] = [];
  const installing = {
    state: "installing",
    postMessage: vi.fn(),
    addEventListener: (_: string, listener: Listener) =>
      statechange.push(listener),
  };
  fake.registration.installing = installing;
  fake.registration.listeners.get("updatefound")?.forEach((run) => run());
  installing.state = "installed";
  statechange.forEach((run) => run());
  expect(onUpdateReady).toHaveBeenCalledTimes(1);
});

test("a failed registration is reported as not registered", async () => {
  const fake = fakeContainer({ controller: true });
  fake.register.mockRejectedValue(new Error("blocked"));
  expect(
    await registerServiceWorker(
      environment({ container: fake.container }),
      vi.fn(),
    ),
  ).toBe(false);
});

test("the update toast says a new version is ready and reloads on request", () => {
  const onReload = vi.fn();
  const onDismiss = vi.fn();
  render(createElement(UpdateNotice, { onReload, onDismiss }));
  expect(screen.getByRole("status").textContent).toContain(
    "A new version of Hito is ready",
  );
  fireEvent.click(screen.getByRole("button", { name: "Reload" }));
  expect(onReload).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Later" }));
  expect(onDismiss).toHaveBeenCalledTimes(1);
});
