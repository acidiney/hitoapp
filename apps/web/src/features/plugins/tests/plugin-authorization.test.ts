import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import PluginSettings from "../components/PluginSettings";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { pluginsApi } from "../api/pluginsApi";
import { grantedAccess } from "../model/pluginSummary";

vi.mock("../api/pluginsApi", () => ({
  pluginsApi: {
    settings: vi.fn(),
    checkDeclaration: vi.fn(),
    connectionStatus: vi.fn(),
    beginAuthorization: vi.fn(),
    authorizationStatus: vi.fn(),
    disconnect: vi.fn(),
  },
}));

const providerUrl = "https://accounts.example/authorize?client_id=c&state=s";
const plugin: Schema<"PluginInstallation"> = {
  id: "installation",
  service_id: "service",
  revision: 4,
  checked_at: 1,
  credential_set: false,
  manifest: {
    id: "org.hito.fixture",
    name: "Fixture provider",
    protocol_version: 1,
    capabilities: ["playlists"],
    authorization: {
      kind: "oauth2_pkce",
      client_id: "client",
      authorization_endpoint: "https://accounts.example/authorize",
      token_endpoint: "https://accounts.example/token",
      scopes: ["playlist-read-private"],
    },
  },
};
const plain: Schema<"PluginInstallation"> = {
  ...plugin,
  id: "plain",
  manifest: { ...plugin.manifest, authorization: undefined },
};
const notConnected: Schema<"PluginConnectionStatus"> = {
  status: "not_connected",
  scopes: [],
};
const connected: Schema<"PluginConnectionStatus"> = {
  status: "connected",
  scopes: ["playlist-read-private", "user-library-read"],
  expires_at: 4_000_000_000,
};
const attempt = (expires = Math.floor(Date.now() / 1000) + 600) => ({
  id: "attempt-1",
  authorization_url: providerUrl,
  expires_at: expires,
});

function settings(installations: Schema<"PluginInstallation">[]) {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    services: [],
    installations,
    credential_storage_ready: true,
  });
}
function page(account = "alice") {
  return createElement(ArtworkProvider, {
    account,
    children: createElement(PluginSettings, { account, admin: false }),
  });
}
function popup() {
  return {
    opener: {} as unknown,
    closed: false,
    close: vi.fn(),
    location: { href: "about:blank" },
  };
}
async function flush() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
}
async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}
async function ready(initialStatus = notConnected) {
  vi.mocked(pluginsApi.connectionStatus).mockResolvedValue(initialStatus);
  const view = render(page());
  await screen.findByText("Fixture provider");
  await waitFor(() =>
    expect(status().textContent).not.toBe("Checking sign-in…"),
  );
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  return view;
}
async function connect() {
  fireEvent.click(screen.getByRole("button", { name: "Connect" }));
  await flush();
}
const status = () => screen.getByRole("status", { name: "Sign-in status" });

beforeEach(() => {
  vi.resetAllMocks();
  settings([plugin]);
  vi.mocked(pluginsApi.beginAuthorization).mockResolvedValue(attempt());
  vi.mocked(pluginsApi.authorizationStatus).mockResolvedValue({
    id: "attempt-1",
    status: "pending",
    expires_at: attempt().expires_at,
  });
  vi.mocked(pluginsApi.disconnect).mockResolvedValue(undefined);
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

test("plugins without an authorization declaration show no sign-in controls", async () => {
  settings([plain]);
  render(page());
  await screen.findByText("Fixture provider");
  expect(screen.queryByRole("button", { name: "Connect" })).toBeNull();
  expect(screen.queryByText("Not connected")).toBeNull();
  expect(pluginsApi.connectionStatus).not.toHaveBeenCalled();
});

test("a plugin that is not connected offers Connect", async () => {
  await ready();
  expect(status().textContent).toBe("Not connected");
  expect(screen.getByRole("button", { name: "Connect" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Disconnect" })).toBeNull();
  expect(pluginsApi.connectionStatus).toHaveBeenCalledWith(
    "alice",
    "installation",
    expect.any(AbortSignal),
  );
});

test("a connected plugin lists granted access in plain words and offers Disconnect", async () => {
  await ready(connected);
  expect(status().textContent).toBe("Connected");
  expect(
    screen.getByText("Access: playlist read private, user library read"),
  ).toBeTruthy();
  expect(screen.getByRole("button", { name: "Disconnect" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Connect" })).toBeNull();
  expect(document.body.textContent).not.toContain("4000000000");
});

test("a connection that needs sign-in again offers Reconnect", async () => {
  await ready({ status: "needs_reauthorization", scopes: [] });
  expect(status().textContent).toBe("Sign in again needed");
  expect(screen.getByRole("button", { name: "Reconnect" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Disconnect" })).toBeTruthy();
});

test("Connect opens the provider in a new tab, waits, polls with backoff and shows Connected", async () => {
  const tab = popup();
  const open = vi
    .spyOn(window, "open")
    .mockReturnValue(tab as unknown as Window);
  await ready();
  await connect();

  expect(open).toHaveBeenCalledExactlyOnceWith("", "_blank");
  expect(tab.opener).toBeNull();
  expect(pluginsApi.beginAuthorization).toHaveBeenCalledExactlyOnceWith(
    "alice",
    plugin,
    expect.any(AbortSignal),
  );
  expect(tab.location.href).toBe(providerUrl);
  expect(status().textContent).toBe("Waiting for Fixture provider…");
  expect(screen.queryByRole("button", { name: "Connect" })).toBeNull();
  expect(window.location.href).not.toContain("attempt-1");
  expect(pluginsApi.authorizationStatus).not.toHaveBeenCalled();

  await advance(1000);
  expect(pluginsApi.authorizationStatus).toHaveBeenCalledTimes(1);
  expect(pluginsApi.authorizationStatus).toHaveBeenCalledWith(
    "alice",
    "installation",
    "attempt-1",
    expect.any(AbortSignal),
  );
  await advance(1000);
  expect(pluginsApi.authorizationStatus).toHaveBeenCalledTimes(1);
  await advance(500);
  expect(pluginsApi.authorizationStatus).toHaveBeenCalledTimes(2);

  vi.mocked(pluginsApi.authorizationStatus).mockResolvedValue({
    id: "attempt-1",
    status: "completed",
    expires_at: attempt().expires_at,
  });
  vi.mocked(pluginsApi.connectionStatus).mockResolvedValue(connected);
  await advance(5000);
  expect(status().textContent).toBe("Connected");
  const polls = vi.mocked(pluginsApi.authorizationStatus).mock.calls.length;
  await advance(60_000);
  expect(vi.mocked(pluginsApi.authorizationStatus).mock.calls.length).toBe(
    polls,
  );
});

test("an exchanging attempt keeps waiting", async () => {
  vi.spyOn(window, "open").mockReturnValue(popup() as unknown as Window);
  await ready();
  await connect();
  vi.mocked(pluginsApi.authorizationStatus).mockResolvedValue({
    id: "attempt-1",
    status: "exchanging",
    expires_at: attempt().expires_at,
  });
  await advance(1000);
  expect(status().textContent).toBe("Waiting for Fixture provider…");
});

test("a failed attempt explains itself and allows another try", async () => {
  vi.spyOn(window, "open").mockReturnValue(popup() as unknown as Window);
  await ready();
  await connect();
  vi.mocked(pluginsApi.authorizationStatus).mockResolvedValue({
    id: "attempt-1",
    status: "failed",
    expires_at: attempt().expires_at,
  });
  await advance(1000);
  expect(status().textContent).toBe("Not connected");
  expect(screen.getByText(/did not finish/).textContent).toContain(
    "Fixture provider",
  );
  expect(screen.getByRole("button", { name: "Connect" })).toBeTruthy();
});

test("an attempt that outlives its expiry stops polling and says so", async () => {
  vi.spyOn(window, "open").mockReturnValue(popup() as unknown as Window);
  await ready();
  vi.mocked(pluginsApi.beginAuthorization).mockResolvedValue(
    attempt(Math.floor(Date.now() / 1000) + 3),
  );
  await connect();
  await advance(10_000);
  expect(status().textContent).toBe("Not connected");
  expect(screen.getByText(/timed out/)).toBeTruthy();
  const polls = vi.mocked(pluginsApi.authorizationStatus).mock.calls.length;
  await advance(60_000);
  expect(vi.mocked(pluginsApi.authorizationStatus).mock.calls.length).toBe(
    polls,
  );
});

test("an attempt the server no longer knows ends the wait", async () => {
  vi.spyOn(window, "open").mockReturnValue(popup() as unknown as Window);
  await ready();
  await connect();
  const { HttpError } = await import("../../../shared/http");
  vi.mocked(pluginsApi.authorizationStatus).mockRejectedValue(
    new HttpError(404, "Not found"),
  );
  await advance(1000);
  expect(status().textContent).toBe("Not connected");
  expect(screen.getByText(/could not be completed/)).toBeTruthy();
});

test("a transient status error keeps waiting", async () => {
  vi.spyOn(window, "open").mockReturnValue(popup() as unknown as Window);
  await ready();
  await connect();
  vi.mocked(pluginsApi.authorizationStatus).mockRejectedValueOnce(
    new Error("offline"),
  );
  await advance(1000);
  expect(status().textContent).toBe("Waiting for Fixture provider…");
  await advance(2000);
  expect(pluginsApi.authorizationStatus).toHaveBeenCalledTimes(2);
});

test("regaining focus or visibility checks the attempt at once", async () => {
  vi.spyOn(window, "open").mockReturnValue(popup() as unknown as Window);
  await ready();
  await connect();
  fireEvent(window, new Event("focus"));
  await flush();
  expect(pluginsApi.authorizationStatus).toHaveBeenCalledTimes(1);
  fireEvent(document, new Event("visibilitychange"));
  await flush();
  expect(pluginsApi.authorizationStatus).toHaveBeenCalledTimes(2);
});

test("a blocked new tab falls back to a link the user can open", async () => {
  vi.spyOn(window, "open").mockReturnValue(null);
  await ready();
  await connect();
  const link = screen.getByRole("link", {
    name: "Open Fixture provider sign-in",
  });
  expect(link.getAttribute("href")).toBe(providerUrl);
  expect(link.getAttribute("target")).toBe("_blank");
  expect(link.getAttribute("rel")).toBe("noopener noreferrer");
  expect(status().textContent).toBe("Waiting for Fixture provider…");
});

test("a failed begin closes the pre-opened tab and shows the reason", async () => {
  const tab = popup();
  vi.spyOn(window, "open").mockReturnValue(tab as unknown as Window);
  await ready();
  vi.mocked(pluginsApi.beginAuthorization).mockRejectedValue(
    new Error("Too many sign-in attempts"),
  );
  await connect();
  expect(tab.close).toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toContain(
    "Too many sign-in attempts",
  );
  expect(screen.getByRole("button", { name: "Connect" })).toBeTruthy();
});

test("Stop waiting cancels polling and returns to Connect", async () => {
  vi.spyOn(window, "open").mockReturnValue(popup() as unknown as Window);
  await ready();
  await connect();
  fireEvent.click(screen.getByRole("button", { name: "Stop waiting" }));
  await advance(30_000);
  expect(pluginsApi.authorizationStatus).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Connect" })).toBeTruthy();
});

test("Disconnect asks first, cancels cleanly, then deletes and refreshes", async () => {
  await ready(connected);
  fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
  const dialog = screen.getByRole("dialog", {
    name: "Disconnect Fixture provider?",
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(pluginsApi.disconnect).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
  vi.mocked(pluginsApi.connectionStatus).mockResolvedValue(notConnected);
  fireEvent.click(
    within(
      screen.getByRole("dialog", { name: "Disconnect Fixture provider?" }),
    ).getByRole("button", { name: "Disconnect" }),
  );
  await flush();
  expect(pluginsApi.disconnect).toHaveBeenCalledExactlyOnceWith(
    "alice",
    "installation",
    expect.any(AbortSignal),
  );
  expect(status().textContent).toBe("Not connected");
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("a status check that fails can be repeated", async () => {
  vi.mocked(pluginsApi.connectionStatus).mockRejectedValueOnce(
    new Error("offline"),
  );
  render(page());
  const again = await screen.findByRole("button", { name: "Check again" });
  vi.mocked(pluginsApi.connectionStatus).mockResolvedValue(notConnected);
  fireEvent.click(again);
  await waitFor(() => expect(screen.getByText("Not connected")).toBeTruthy());
});

test("unmounting stops polling and cancels the request", async () => {
  vi.spyOn(window, "open").mockReturnValue(popup() as unknown as Window);
  const view = await ready();
  await connect();
  await advance(1000);
  const signal = vi.mocked(pluginsApi.authorizationStatus).mock
    .calls[0]![3] as AbortSignal;
  view.unmount();
  expect(signal.aborted).toBe(true);
  const polls = vi.mocked(pluginsApi.authorizationStatus).mock.calls.length;
  fireEvent(window, new Event("focus"));
  await advance(60_000);
  expect(vi.mocked(pluginsApi.authorizationStatus).mock.calls.length).toBe(
    polls,
  );
});

test("switching accounts stops polling for the previous account", async () => {
  vi.spyOn(window, "open").mockReturnValue(popup() as unknown as Window);
  const view = await ready();
  await connect();
  await advance(1000);
  const signal = vi.mocked(pluginsApi.authorizationStatus).mock
    .calls[0]![3] as AbortSignal;
  vi.mocked(pluginsApi.connectionStatus).mockResolvedValue(notConnected);
  view.rerender(page("bob"));
  await flush();
  expect(signal.aborted).toBe(true);
  const polls = vi.mocked(pluginsApi.authorizationStatus).mock.calls.length;
  await advance(60_000);
  expect(vi.mocked(pluginsApi.authorizationStatus).mock.calls.length).toBe(
    polls,
  );
  expect(pluginsApi.connectionStatus).toHaveBeenLastCalledWith(
    "bob",
    "installation",
    expect.any(AbortSignal),
  );
});

test("granted scopes become plain words", () => {
  expect(grantedAccess([])).toBe("");
  expect(
    grantedAccess(["a-b_c", "https://x.example/auth/youtube.readonly"]),
  ).toBe("a b c, youtube readonly");
});
