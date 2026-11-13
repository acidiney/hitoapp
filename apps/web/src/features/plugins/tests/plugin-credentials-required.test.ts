import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import PluginSettings from "../components/PluginSettings";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { pluginsApi } from "../api/pluginsApi";
import {
  reportCredentialRequirement,
  resetCredentialRequirements,
} from "../model/credentialRequirements";
import { onInvalidated } from "../../../shared/invalidation";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

const real =
  await vi.importActual<typeof import("../api/pluginsApi")>(
    "../api/pluginsApi",
  );

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

function installation(
  id: string,
  name: string,
  revision: number,
  health?: Schema<"PluginHealth">,
): Schema<"PluginInstallation"> {
  return {
    id,
    service_id: service.id,
    revision,
    checked_at: 1,
    credential_set: true,
    manifest: {
      id: `org.hito.${id}`,
      name,
      protocol_version: 1,
      capabilities: ["search", "sources"],
    },
    ...(health ? { health } : {}),
  };
}

const wireHealth = (state: Schema<"PluginHealthState">) =>
  ({ state }) satisfies Schema<"PluginHealth">;

function show(...items: Schema<"PluginInstallation">[]) {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    services: [service],
    installations: items,
    credential_storage_ready: true,
  });
  render(
    createElement(ArtworkProvider, {
      account: "bob",
      children: createElement(PluginSettings, { account: "bob", admin: false }),
    }),
  );
}

function card(name: string) {
  return screen.getByRole("heading", { name }).closest("li") as HTMLElement;
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(pluginsApi.checkDeclaration).mockResolvedValue({ kind: "current" });
});

afterEach(() => {
  cleanup();
  resetCredentialRequirements();
  vi.unstubAllGlobals();
});

test("a stamped error marks only the affected connection and leaves its peer connected", async () => {
  show(installation("a", "Lyrics A", 3), installation("b", "Lyrics B", 5));
  await screen.findByRole("heading", { name: "Lyrics A" });
  act(() =>
    reportCredentialRequirement("bob", {
      stamp: { installationId: "a", installationRevision: 3 },
    }),
  );
  expect(card("Lyrics A").textContent).toContain("Credentials need updating");
  expect(card("Lyrics A").textContent).toContain(
    "Lyrics A needs its credentials updated.",
  );
  expect(card("Lyrics B").textContent).toContain("Connected");
  expect(card("Lyrics B").textContent).not.toContain("credentials");
});

test("a stale stamp for an older revision, another account or no connection marks nothing", async () => {
  show(installation("a", "Lyrics A", 4), installation("b", "Lyrics B", 5));
  await screen.findByRole("heading", { name: "Lyrics A" });
  act(() => {
    reportCredentialRequirement("bob", {
      stamp: { installationId: "a", installationRevision: 3 },
    });
    reportCredentialRequirement("alice", {
      stamp: { installationId: "b", installationRevision: 5 },
    });
    reportCredentialRequirement("bob", { stamp: null });
    reportCredentialRequirement("bob", {
      stamp: { installationId: "gone", installationRevision: 1 },
    });
  });
  expect(screen.queryByText(/Credentials need updating/)).toBeNull();
  expect(document.body.textContent).not.toContain("needs its credentials");
});

test("a late rejection from an older revision keeps the warning for the current revision", async () => {
  show(installation("a", "Lyrics A", 4));
  await screen.findByRole("heading", { name: "Lyrics A" });
  act(() => {
    reportCredentialRequirement("bob", {
      stamp: { installationId: "a", installationRevision: 4 },
    });
    reportCredentialRequirement("bob", {
      stamp: { installationId: "a", installationRevision: 3 },
    });
  });
  expect(card("Lyrics A").textContent).toContain("Credentials need updating");
  expect(
    screen.getByRole("button", { name: "Update credentials for Lyrics A" }),
  ).toBeTruthy();
});

test("the credentials_required health value renders for that connection only, and a missing health value is not treated as proof of success", async () => {
  show(
    installation("a", "Lyrics A", 3, wireHealth("credentials_required")),
    installation("b", "Lyrics B", 5),
    installation("c", "Lyrics C", 1, wireHealth("ok")),
  );
  await screen.findByRole("heading", { name: "Lyrics A" });
  expect(card("Lyrics A").textContent).toContain("Credentials need updating");
  expect(card("Lyrics B").textContent).not.toContain("answering");
  expect(card("Lyrics B").textContent).not.toContain("credentials");
  expect(card("Lyrics C").textContent).toContain("Connected and answering");
});

test("Update credentials opens the existing token form and never shows a saved token", async () => {
  show(installation("a", "Lyrics A", 3, wireHealth("credentials_required")));
  await screen.findByRole("heading", { name: "Lyrics A" });
  fireEvent.click(
    screen.getByRole("button", { name: "Update credentials for Lyrics A" }),
  );
  const field = screen.getByLabelText<HTMLInputElement>("New bearer token");
  expect(field.type).toBe("password");
  expect(screen.getByRole("button", { name: "Save token" })).toBeTruthy();
  expect(document.body.textContent).not.toMatch(/bearer-secret/);
});

test("a successful credential update clears the mark and announces plugin and lyric changes once", async () => {
  vi.mocked(pluginsApi.refresh).mockImplementation(
    real.pluginsApi.refresh.bind(real.pluginsApi),
  );
  const refreshed = installation("a", "Lyrics A", 4);
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) =>
      path.endsWith("/refresh")
        ? Response.json({ kind: "updated", installation: refreshed })
        : Response.json({ error: "Not found" }, { status: 404 }),
    ),
  );
  const plugins = vi.fn();
  const lyrics = vi.fn();
  const stops = [
    onInvalidated("plugins", plugins),
    onInvalidated("lyrics", lyrics),
  ];
  show(installation("a", "Lyrics A", 3));
  await screen.findByRole("heading", { name: "Lyrics A" });
  act(() =>
    reportCredentialRequirement("bob", {
      stamp: { installationId: "a", installationRevision: 3 },
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Update credentials for Lyrics A" }),
  );
  fireEvent.change(screen.getByLabelText("New bearer token"), {
    target: { value: "bearer-secret-value" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save token" }));
  await waitFor(() =>
    expect(screen.queryByText(/Credentials need updating/)).toBeNull(),
  );
  expect(plugins).toHaveBeenCalledTimes(1);
  expect(plugins).toHaveBeenCalledWith("a", undefined);
  expect(lyrics).toHaveBeenCalledTimes(1);
  expect(document.body.textContent).not.toContain("bearer-secret-value");
  stops.forEach((stop) => stop());
});

test("a refresh that keeps the saved token does not announce a credential update", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () =>
      Response.json({
        kind: "updated",
        installation: installation("a", "Lyrics A", 4),
      }),
    ),
  );
  const lyrics = vi.fn();
  const stop = onInvalidated("lyrics", lyrics);
  await real.pluginsApi.refresh(
    "bob",
    installation("a", "Lyrics A", 3),
    { revision: 3, credential: { action: "keep" } },
    new AbortController().signal,
  );
  expect(lyrics).not.toHaveBeenCalled();
  stop();
});
