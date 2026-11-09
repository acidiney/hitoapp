import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { render, screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import PluginSettings from "../components/PluginSettings";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { pluginsApi } from "../api/pluginsApi";
import { pluginHealthNotice } from "../model/pluginHealth";

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
    name: "YouTube",
    protocol_version: 1,
    capabilities: ["search", "sources"],
  },
};

function withHealth(health?: Schema<"PluginHealth">) {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    services: [service],
    installations: [health ? { ...plugin, health } : plugin],
    credential_storage_ready: true,
  });
}

function show() {
  render(
    createElement(ArtworkProvider, {
      account: "bob",
      children: createElement(PluginSettings, { account: "bob", admin: false }),
    }),
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(pluginsApi.checkDeclaration).mockResolvedValue({ kind: "current" });
});

test("a plugin that cannot reach its provider says so by the provider's name and what to check", async () => {
  withHealth({ state: "degraded", reason: "network", since: 1_700_000_000 });
  show();
  expect(await screen.findByText("Hito cannot reach YouTube.")).toBeTruthy();
  expect(
    screen.getByText("Check the server's internet connection or DNS."),
  ).toBeTruthy();
});

test("a provider that refuses requests is described without blaming the network", async () => {
  withHealth({ state: "degraded", reason: "refused", since: 1_700_000_000 });
  show();
  expect(
    await screen.findByText("YouTube is refusing requests right now."),
  ).toBeTruthy();
  expect(screen.getByText("Try again later.")).toBeTruthy();
  expect(screen.queryByText(/internet connection/)).toBeNull();
});

test("a plugin service that does not answer is reported as not responding", async () => {
  withHealth({ state: "unreachable" });
  show();
  expect(
    await screen.findByText("YouTube is not responding right now."),
  ).toBeTruthy();
  expect(screen.getByText("Try again in a moment.")).toBeTruthy();
});

test("healthy plugins and plugins without a health report show no warning", async () => {
  for (const health of [{ state: "ok" as const }, undefined]) {
    withHealth(health);
    const view = render(
      createElement(ArtworkProvider, {
        account: "bob",
        children: createElement(PluginSettings, {
          account: "bob",
          admin: false,
        }),
      }),
    );
    await screen.findByText("Finds songs and plays them from YouTube.");
    expect(
      screen.queryByText(/cannot reach|refusing|not responding/),
    ).toBeNull();
    view.unmount();
  }
});

test("the notice never repeats the private since time or any reason text from the service", () => {
  const notice = pluginHealthNotice("YouTube", {
    state: "degraded",
    reason: "network",
    since: 1_700_000_000,
  });
  expect(JSON.stringify(notice)).not.toMatch(/1700000000|2023/);
  expect(pluginHealthNotice("YouTube", { state: "ok" })).toBeNull();
  expect(pluginHealthNotice("YouTube", undefined)).toBeNull();
  expect(pluginHealthNotice("YouTube", null)).toBeNull();
});
