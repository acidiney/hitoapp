// @vitest-environment jsdom
import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import PluginSettings from "./components/plugins/PluginSettings";
import { ArtworkProvider } from "./ArtworkProvider";
import { pluginsApi } from "./pluginsApi";
import type { PluginInstallation, PluginSettings as Settings } from "./types";
vi.mock("./pluginsApi", () => ({
  pluginsApi: {
    settings: vi.fn(),
    install: vi.fn(),
    offer: vi.fn(),
    withdraw: vi.fn(),
    refresh: vi.fn(),
    remove: vi.fn(),
    clearCredential: vi.fn(),
  },
}));
const service = {
  id: "service",
  name: "Community service",
  base_url: "https://plugin.example/",
  revision: 1,
};
const plugin: PluginInstallation = {
  id: "installation",
  service_id: service.id,
  revision: 1,
  checked_at: 1,
  credential_set: true,
  manifest: {
    id: "org.hito.fixture",
    name: "Fixture provider",
    protocol_version: 1,
    capabilities: ["search", "sources"],
  },
};
const initial: Settings = {
  services: [service],
  installations: [],
  credential_storage_ready: true,
};
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(pluginsApi.settings).mockResolvedValue(initial);
});
afterEach(cleanup);

function settingsPage(account: string, admin: boolean) {
  return createElement(ArtworkProvider, {
    account,
    children: createElement(PluginSettings, { account, admin }),
  });
}

test("administrators offer an address and connect separately using its manifest name", async () => {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    services: [],
  });
  vi.mocked(pluginsApi.offer).mockResolvedValue({
    ...service,
    name: "Fixture provider",
  });
  render(settingsPage("admin", true));
  fireEvent.click(await screen.findByText("Services for this server"));

  const address = screen.getByRole("textbox", { name: "Service address" });
  expect(screen.queryByRole("textbox", { name: "Service name" })).toBeNull();
  expect(
    address.closest("form")?.querySelectorAll("input:required"),
  ).toHaveLength(1);
  expect((address as HTMLInputElement).required).toBe(true);
  fireEvent.change(address, {
    target: { value: service.base_url },
  });
  fireEvent.click(screen.getByRole("button", { name: "Offer service" }));

  await screen.findByRole("option", { name: "Fixture provider" });
  expect(pluginsApi.offer).toHaveBeenCalledExactlyOnceWith(
    "admin",
    { base_url: service.base_url },
    expect.any(AbortSignal),
  );
  expect(screen.getByLabelText("Service").textContent).toBe("Fixture provider");
  expect(screen.getByRole("button", { name: "Connect service" })).toBeTruthy();
  expect(pluginsApi.install).not.toHaveBeenCalled();
  expect(
    screen.getByText("No plugins connected to this account."),
  ).toBeTruthy();
  expect((address as HTMLInputElement).value).toBe("");
});

test("invalid nonempty custom labels reach server validation and retain the draft", async () => {
  const name = "\u00a0".repeat(129);
  const validationError =
    "Use a service name of 1 to 256 bytes without control characters";
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    services: [],
  });
  vi.mocked(pluginsApi.offer).mockRejectedValue(new Error(validationError));
  render(settingsPage("admin", true));
  fireEvent.click(await screen.findByText("Services for this server"));
  fireEvent.click(screen.getByText("Advanced settings"));
  const label = screen.getByRole("textbox", {
    name: "Custom label (optional)",
  });
  fireEvent.change(label, { target: { value: name } });
  fireEvent.change(screen.getByRole("textbox", { name: "Service address" }), {
    target: { value: service.base_url },
  });
  fireEvent.click(screen.getByRole("button", { name: "Offer service" }));

  await screen.findByRole("alert");
  expect(pluginsApi.offer).toHaveBeenCalledExactlyOnceWith(
    "admin",
    { base_url: service.base_url, name },
    expect.any(AbortSignal),
  );
  expect(screen.getByRole("alert").textContent).toContain(validationError);
  expect((label as HTMLInputElement).value).toBe(name);
  expect(screen.queryByRole("button", { name: "Connect service" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Withdraw service" })).toBeNull();
  expect(pluginsApi.install).not.toHaveBeenCalled();
});

test("ordinary accounts connect offered services and retain failed token drafts", async () => {
  vi.mocked(pluginsApi.install)
    .mockRejectedValueOnce(new Error("Plugin unavailable"))
    .mockResolvedValue(plugin);
  render(settingsPage("bob", false));
  expect(
    screen.getByRole("link", { name: "Discover" }).getAttribute("href"),
  ).toBe("/discover");
  expect(
    screen.queryByText("Provider content is not available in Hito yet."),
  ).toBeNull();
  const input = await screen.findByLabelText("Bearer token (optional)");
  fireEvent.change(input, { target: { value: "private-token" } });
  fireEvent.click(screen.getByRole("button", { name: "Connect service" }));
  await screen.findByRole("alert");
  expect((input as HTMLInputElement).value).toBe("private-token");
  expect(screen.queryByText("Services for this server")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Connect service" }));
  await screen.findByRole("heading", { name: "Fixture provider" });
  expect(pluginsApi.install).toHaveBeenLastCalledWith(
    "bob",
    {
      service_id: "service",
      service_revision: 1,
      bearer_token: "private-token",
    },
    expect.any(AbortSignal),
  );
  expect(screen.queryByLabelText("Bearer token (optional)")).toBeNull();
});

test("manifest changes are reviewed before accepting new capabilities", async () => {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    installations: [plugin],
  });
  const manifest = { ...plugin.manifest, capabilities: ["metadata" as const] };
  vi.mocked(pluginsApi.refresh)
    .mockResolvedValueOnce({
      kind: "review_required",
      manifest,
      digest: "reviewed-digest",
    })
    .mockResolvedValue({
      kind: "updated",
      installation: { ...plugin, revision: 2, manifest },
    });
  render(settingsPage("bob", false));
  fireEvent.click(
    await screen.findByRole("button", { name: "Check connection" }),
  );
  await screen.findByRole("heading", { name: "Review plugin changes" });
  expect(pluginsApi.refresh).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Accept changes" }));
  await waitFor(() =>
    expect(
      screen.queryByRole("heading", { name: "Review plugin changes" }),
    ).toBeNull(),
  );
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Check connection" }),
  );
  expect(pluginsApi.refresh).toHaveBeenLastCalledWith(
    "bob",
    plugin,
    {
      revision: 1,
      accept_manifest_digest: "reviewed-digest",
      credential: { action: "keep" },
    },
    expect.any(AbortSignal),
  );
});

test("old account responses cannot add private connections after an account change", async () => {
  let resolve!: (plugin: PluginInstallation) => void;
  vi.mocked(pluginsApi.install).mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  const view = render(settingsPage("alice", true));
  fireEvent.click(
    await screen.findByRole("button", { name: "Connect service" }),
  );
  await waitFor(() => expect(pluginsApi.install).toHaveBeenCalledTimes(1));
  const signal = vi.mocked(pluginsApi.install).mock.calls[0][2];
  view.rerender(settingsPage("bob", false));
  await screen.findByRole("button", { name: "Connect service" });
  await act(async () => resolve(plugin));
  await waitFor(() => expect(signal.aborted).toBe(true));
  expect(
    screen.queryByRole("heading", { name: "Fixture provider" }),
  ).toBeNull();
});

test("unrelated service offers do not erase a failed token draft on reload", async () => {
  vi.mocked(pluginsApi.install).mockRejectedValue(
    new Error("Plugin unavailable"),
  );
  render(settingsPage("bob", false));
  fireEvent.change(await screen.findByLabelText("Bearer token (optional)"), {
    target: { value: "failed-private-draft" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Connect service" }));
  await screen.findByRole("alert");
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    services: [{ ...service, id: "other", name: "Other service" }, service],
  });
  fireEvent.click(screen.getByRole("button", { name: "Reload connections" }));
  await screen.findByRole("option", { name: "Other service" });
  expect(
    (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
      .value,
  ).toBe("failed-private-draft");
});

test("withdrawing the selected offer clears its token before selecting another service", async () => {
  vi.mocked(pluginsApi.install).mockRejectedValue(new Error("Offer withdrawn"));
  render(settingsPage("bob", false));
  fireEvent.change(await screen.findByLabelText("Bearer token (optional)"), {
    target: { value: "old-service-private-draft" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Connect service" }));
  await screen.findByRole("alert");
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    services: [{ ...service, id: "other", name: "Other service" }],
  });
  fireEvent.click(screen.getByRole("button", { name: "Reload connections" }));
  await screen.findByRole("option", { name: "Other service" });
  expect(
    (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
      .value,
  ).toBe("");
});

test("a delayed settings reload cannot remove a newly accepted connection", async () => {
  vi.mocked(pluginsApi.install)
    .mockRejectedValueOnce(new Error("Unavailable"))
    .mockResolvedValue(plugin);
  render(settingsPage("bob", false));
  fireEvent.click(
    await screen.findByRole("button", { name: "Connect service" }),
  );
  await screen.findByRole("alert");
  let finish!: (settings: Settings) => void;
  vi.mocked(pluginsApi.settings).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Reload connections" }));
  await waitFor(() => expect(pluginsApi.settings).toHaveBeenCalledTimes(2));
  fireEvent.click(screen.getByRole("button", { name: "Connect service" }));
  await screen.findByRole("heading", { name: "Fixture provider" });
  await act(async () => finish(initial));
  await waitFor(() =>
    expect(
      screen.getByRole("heading", { name: "Fixture provider" }),
    ).toBeTruthy(),
  );
});
