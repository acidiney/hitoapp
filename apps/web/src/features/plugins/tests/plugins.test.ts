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
import { findCheckConnection } from "../fixtures/pluginInstallation";
import { beforeEach, expect, test, vi } from "vitest";
import PluginSettings from "../components/PluginSettings";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { pluginsApi } from "../api/pluginsApi";

vi.mock("../components/PluginPlaybackOrder", () => ({ default: () => null }));
vi.mock("../api/pluginsApi", () => ({
  pluginsApi: {
    settings: vi.fn(),
    install: vi.fn(),
    preview: vi.fn(),
    offer: vi.fn(),
    withdraw: vi.fn(),
    refresh: vi.fn(),
    checkDeclaration: vi.fn(),
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
const plugin: Schema<"PluginInstallation"> = {
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
const initial: Schema<"PluginSettings"> = {
  services: [service],
  installations: [],
  credential_storage_ready: true,
};
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(pluginsApi.settings).mockResolvedValue(initial);
  vi.mocked(pluginsApi.preview).mockResolvedValue({
    manifest: plugin.manifest,
    digest: "reviewed-installation-digest",
  });
});
function settingsPage(account: string, admin: boolean) {
  return createElement(ArtworkProvider, {
    account,
    children: createElement(PluginSettings, { account, admin }),
  });
}

async function openConnect(name = service.name) {
  fireEvent.click(
    await screen.findByRole("button", { name: `Connect ${name}` }),
  );
}

async function openServerTab() {
  fireEvent.click(await screen.findByRole("button", { name: /This server/ }));
}

async function reviewAndAccept() {
  if (!screen.queryByRole("button", { name: "Review connection" }))
    await openConnect();
  fireEvent.click(
    await screen.findByRole("button", { name: "Review connection" }),
  );
  await screen.findByRole("heading", { name: "Review connection" });
  fireEvent.click(screen.getByRole("button", { name: "Accept and connect" }));
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
  await openServerTab();

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

  await screen.findByRole("heading", { name: "Fixture provider" });
  expect(pluginsApi.offer).toHaveBeenCalledExactlyOnceWith(
    "admin",
    { base_url: service.base_url },
    expect.any(AbortSignal),
  );
  expect(
    screen.getByRole("button", { name: "Withdraw Fixture provider" }),
  ).toBeTruthy();
  expect(pluginsApi.install).not.toHaveBeenCalled();
  expect((address as HTMLInputElement).value).toBe("");
  fireEvent.click(screen.getByRole("button", { name: "Your plugins" }));
  expect(
    screen.getByRole("button", { name: "Connect Fixture provider" }),
  ).toBeTruthy();
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
  await openServerTab();
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
  const alert = screen.getByRole("alert");
  expect(alert.textContent).toContain(validationError);
  expect(alert.closest(".plugin-offer")).not.toBeNull();
  expect(
    screen
      .getByRole("textbox", { name: "Service address" })
      .getAttribute("aria-invalid"),
  ).toBe("true");
  expect(
    screen.queryByRole("button", { name: "Reload connections" }),
  ).toBeNull();
  expect((label as HTMLInputElement).value).toBe(name);
  expect(
    screen.queryByRole("button", { name: "Review connection" }),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: /^Withdraw/ })).toBeNull();
  expect(pluginsApi.install).not.toHaveBeenCalled();
});

test("ordinary accounts connect offered services and retain failed token drafts", async () => {
  vi.mocked(pluginsApi.install)
    .mockRejectedValueOnce(new Error("Plugin unavailable"))
    .mockResolvedValue(plugin);
  render(settingsPage("bob", false));
  expect(screen.queryByRole("link", { name: "Discover" })).toBeNull();
  expect(
    screen.queryByText("Provider content is not available in Hito yet."),
  ).toBeNull();
  await openConnect();
  const input = await screen.findByLabelText("Bearer token (optional)");
  fireEvent.change(input, { target: { value: "private-token" } });
  await reviewAndAccept();
  await screen.findByRole("alert");
  expect((input as HTMLInputElement).value).toBe("private-token");
  expect(screen.queryByText("Services for this server")).toBeNull();
  await reviewAndAccept();
  await screen.findByRole("heading", { name: "Fixture provider" });
  expect(pluginsApi.install).toHaveBeenLastCalledWith(
    "bob",
    {
      service_id: "service",
      service_revision: 1,
      bearer_token: "private-token",
      accept_manifest_digest: "reviewed-installation-digest",
    },
    expect.any(AbortSignal),
  );
  expect(screen.queryByLabelText("Bearer token (optional)")).toBeNull();
});

test("an offered service opens its own connection dialog", async () => {
  render(settingsPage("bob", false));
  await openConnect();
  const dialog = await screen.findByRole("dialog", {
    name: `Connect ${service.name}`,
  });
  expect(
    within(dialog).getByRole("button", { name: "Review connection" }),
  ).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("the card does not repeat the section intro and no sentence floats between cards", async () => {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    services: [],
  });
  const view = render(settingsPage("admin", true));
  await openServerTab();
  expect(await screen.findByText(/Nothing is offered yet/)).toBeTruthy();
  expect(view.container.textContent).not.toMatch(/Offer a service below/);
  expect(view.container.textContent).not.toMatch(
    /Connect to services offered by your administrator/,
  );
  view.unmount();
  render(settingsPage("bob", false));
  expect(
    await screen.findByText(/Your administrator has not offered any services/),
  ).toBeTruthy();
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
  fireEvent.click(await findCheckConnection());
  await screen.findByRole("dialog");
  expect(pluginsApi.refresh).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: /^Accept / }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
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
  let resolve!: (plugin: Schema<"PluginInstallation">) => void;
  vi.mocked(pluginsApi.install).mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  const view = render(settingsPage("alice", true));
  await reviewAndAccept();
  await waitFor(() => expect(pluginsApi.install).toHaveBeenCalledTimes(1));
  const signal = vi.mocked(pluginsApi.install).mock.calls[0][2];
  view.rerender(settingsPage("bob", false));
  await screen.findByRole("button", { name: `Connect ${service.name}` });
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
  await openConnect();
  fireEvent.change(await screen.findByLabelText("Bearer token (optional)"), {
    target: { value: "failed-private-draft" },
  });
  await reviewAndAccept();
  await screen.findByRole("alert");
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    services: [{ ...service, id: "other", name: "Other service" }, service],
  });
  fireEvent.click(screen.getByRole("button", { name: "Reload connections" }));
  await screen.findByRole("button", { name: "Connect Other service" });
  expect(
    (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
      .value,
  ).toBe("failed-private-draft");
});

test("withdrawing the selected offer clears its token before selecting another service", async () => {
  vi.mocked(pluginsApi.install).mockRejectedValue(new Error("Offer withdrawn"));
  render(settingsPage("bob", false));
  await openConnect();
  fireEvent.change(await screen.findByLabelText("Bearer token (optional)"), {
    target: { value: "old-service-private-draft" },
  });
  await reviewAndAccept();
  await screen.findByRole("alert");
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    services: [{ ...service, id: "other", name: "Other service" }],
  });
  fireEvent.click(screen.getByRole("button", { name: "Reload connections" }));
  await waitFor(() =>
    expect(screen.queryByLabelText("Bearer token (optional)")).toBeNull(),
  );
  await openConnect("Other service");
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
  await reviewAndAccept();
  await screen.findByRole("alert");
  let finish!: (settings: Schema<"PluginSettings">) => void;
  vi.mocked(pluginsApi.settings).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Reload connections" }));
  await waitFor(() => expect(pluginsApi.settings).toHaveBeenCalledTimes(2));
  await reviewAndAccept();
  await screen.findByRole("heading", { name: "Fixture provider" });
  await act(async () => finish(initial));
  await waitFor(() =>
    expect(
      screen.getByRole("heading", { name: "Fixture provider" }),
    ).toBeTruthy(),
  );
});

test("a connection says in plain words what it does and keeps the technical declaration under Manage", async () => {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    installations: [plugin],
  });
  vi.mocked(pluginsApi.checkDeclaration).mockResolvedValue({ kind: "current" });
  render(settingsPage("bob", false));
  await screen.findByText("Finds songs and plays them from Fixture provider.");
  expect(screen.queryByText("What it does for you")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Manage" }));
  expect(screen.getByText("What it does for you")).toBeTruthy();
  expect(screen.queryByText(/Declared capabilities/)).toBeNull();
  expect(screen.getByText(/No persistence declaration/)).toBeTruthy();
});

test("new capabilities waiting for approval are announced without pressing Check connection", async () => {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    installations: [plugin],
  });
  const manifest = {
    ...plugin.manifest,
    capabilities: ["search" as const, "sources" as const, "artwork" as const],
  };
  vi.mocked(pluginsApi.checkDeclaration).mockResolvedValue({
    kind: "review_required",
    manifest,
    digest: "artwork-digest",
  });
  vi.mocked(pluginsApi.refresh).mockResolvedValue({
    kind: "updated",
    installation: { ...plugin, revision: 2, manifest },
  });
  render(settingsPage("bob", false));
  const banner = await screen.findByRole("status", {
    name: "New: album covers",
  });
  expect(pluginsApi.checkDeclaration).toHaveBeenCalledExactlyOnceWith(
    "bob",
    "installation",
    1,
    expect.any(AbortSignal),
  );
  expect(pluginsApi.refresh).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(within(banner).getByRole("button", { name: "Review" }));
  await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button", { name: /^Accept / }));
  await waitFor(() =>
    expect(
      screen.queryByRole("status", { name: "New: album covers" }),
    ).toBeNull(),
  );
  expect(pluginsApi.refresh).toHaveBeenCalledWith(
    "bob",
    plugin,
    {
      revision: 1,
      accept_manifest_digest: "artwork-digest",
      credential: { action: "keep" },
    },
    expect.any(AbortSignal),
  );
});

test("a failed automatic check stays quiet", async () => {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    installations: [plugin],
  });
  vi.mocked(pluginsApi.checkDeclaration).mockRejectedValue(
    new Error("offline"),
  );
  render(settingsPage("bob", false));
  await findCheckConnection();
  await waitFor(() => expect(pluginsApi.checkDeclaration).toHaveBeenCalled());
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.queryByText("offline")).toBeNull();
});

test("each connection is a card with capability chips beside its name and status", async () => {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    installations: [
      plugin,
      {
        ...plugin,
        id: "second",
        credential_set: false,
        manifest: {
          ...plugin.manifest,
          id: "org.hito.second",
          name: "Second provider",
          capabilities: ["lyrics"],
        },
      },
    ],
  });
  vi.mocked(pluginsApi.checkDeclaration).mockResolvedValue({ kind: "current" });
  render(settingsPage("bob", false));
  const first = (
    await screen.findByRole("heading", { name: "Fixture provider" })
  ).closest("li") as HTMLElement;
  const second = screen
    .getByRole("heading", { name: "Second provider" })
    .closest("li") as HTMLElement;
  expect(first.closest("ul")?.className).toContain("plugin-grid");
  const chips = within(first).getByRole("list", { name: "Capabilities" });
  expect(
    within(chips)
      .getAllByRole("listitem")
      .map((item) => item.textContent),
  ).toEqual(["Search", "Playback"]);
  expect(
    within(
      within(second).getByRole("list", { name: "Capabilities" }),
    ).getAllByRole("listitem"),
  ).toHaveLength(1);
  expect(within(first).getByText("Connected")).toBeTruthy();
  expect(within(second).getByText("Connected")).toBeTruthy();
  fireEvent.click(within(first).getByRole("button", { name: "Manage" }));
  fireEvent.click(within(second).getByRole("button", { name: "Manage" }));
  expect(within(first).getByText("Token saved")).toBeTruthy();
  expect(within(second).getByText("No token saved")).toBeTruthy();
  expect(
    within(first).getByRole("button", { name: "Check connection" }),
  ).toBeTruthy();
  expect(within(first).getByRole("button", { name: "Remove" })).toBeTruthy();
});

test("a connection that declares nothing shows no chip list", async () => {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    ...initial,
    installations: [
      { ...plugin, manifest: { ...plugin.manifest, capabilities: [] } },
    ],
  });
  vi.mocked(pluginsApi.checkDeclaration).mockResolvedValue({ kind: "current" });
  render(settingsPage("bob", false));
  await screen.findByRole("heading", { name: "Fixture provider" });
  expect(screen.queryByRole("list", { name: "Capabilities" })).toBeNull();
});
