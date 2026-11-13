import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  render,
  screen,
  waitFor,
  within,
  fireEvent,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import PluginSettings from "../components/PluginSettings";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { pluginsApi } from "../api/pluginsApi";
import {
  clearCredentialRequirementAfterProtectedSuccess,
  credentialMarkVersion,
  reportCredentialRequirement,
  resetCredentialRequirements,
} from "../model/credentialRequirements";
import { onInvalidated } from "../../../shared/invalidation";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

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

type Phase = "before" | "after";

function serveSettings(
  before: Schema<"PluginSettings">,
  after: Schema<"PluginSettings">,
) {
  const state = { phase: "before" as Phase, afterReads: 0 };
  vi.mocked(pluginsApi.settings).mockImplementation(async () => {
    if (state.phase === "after") state.afterReads += 1;
    return state.phase === "after" ? after : before;
  });
  return state;
}

function settings(
  ...items: Schema<"PluginInstallation">[]
): Schema<"PluginSettings"> {
  return {
    services: [service],
    installations: items,
    credential_storage_ready: true,
  };
}

async function settle() {
  for (;;) {
    const calls =
      vi.mocked(pluginsApi.settings).mock.calls.length +
      vi.mocked(pluginsApi.checkDeclaration).mock.calls.length;
    await act(async () => {
      await Promise.allSettled([
        ...vi.mocked(pluginsApi.settings).mock.results.map((result) => result.value),
        ...vi
          .mocked(pluginsApi.checkDeclaration)
          .mock.results.map((result) => result.value),
      ]);
    });
    const now =
      vi.mocked(pluginsApi.settings).mock.calls.length +
      vi.mocked(pluginsApi.checkDeclaration).mock.calls.length;
    if (now === calls) return;
  }
}

function mount() {
  render(
    createElement(ArtworkProvider, {
      account: "bob",
      children: createElement(PluginSettings, { account: "bob", admin: false }),
    }),
  );
}

const reads = () => vi.mocked(pluginsApi.settings).mock.calls.length;

function recover(state: { phase: Phase }) {
  state.phase = "after";
  const version = credentialMarkVersion("bob", "a");
  act(() => clearCredentialRequirementAfterProtectedSuccess("bob", "a", 3, version));
}

test("a fresh protected success re-reads the mounted settings once and clears the stale credentials_required health", async () => {
  const state = serveSettings(
    settings(installation("a", "Lyrics A", 3, wireHealth("credentials_required"))),
    settings(installation("a", "Lyrics A", 3, wireHealth("ok"))),
  );
  const plugins = vi.fn();
  const stop = onInvalidated("plugins", plugins);
  mount();
  await screen.findByRole("heading", { name: "Lyrics A" });
  act(() => reportCredentialRequirement("bob", { stamp: { installationId: "a", installationRevision: 3 } }));
  await settle();
  const baseline = reads();
  expect(card("Lyrics A").textContent).toContain("Credentials need updating");

  recover(state);
  await waitFor(() => expect(state.afterReads).toBe(1));
  await settle();
  expect(reads()).toBe(baseline + 1);
  expect(card("Lyrics A").textContent).not.toContain("Credentials need updating");
  expect(plugins).not.toHaveBeenCalled();
  stop();
});

test("the mounted card stays marked when the post-recovery read still reports credentials_required", async () => {
  const marked = settings(
    installation("a", "Lyrics A", 3, wireHealth("credentials_required")),
  );
  const state = serveSettings(marked, marked);
  mount();
  await screen.findByRole("heading", { name: "Lyrics A" });
  act(() => reportCredentialRequirement("bob", { stamp: { installationId: "a", installationRevision: 3 } }));
  await settle();
  const baseline = reads();

  recover(state);
  await waitFor(() => expect(state.afterReads).toBe(1));
  await settle();
  expect(reads()).toBe(baseline + 1);
  expect(card("Lyrics A").textContent).toContain("Credentials need updating");
});

test("a recovery published while an unrelated plugin removal is pending still re-reads once after the removal and keeps a rejected peer marked", async () => {
  const state = serveSettings(
    settings(
      installation("a", "Lyrics A", 3, wireHealth("credentials_required")),
      installation("b", "Lyrics B", 5, wireHealth("ok")),
      installation("c", "Lyrics C", 2, wireHealth("credentials_required")),
    ),
    settings(
      installation("a", "Lyrics A", 3, wireHealth("ok")),
      installation("c", "Lyrics C", 2, wireHealth("credentials_required")),
    ),
  );
  let release: () => void = () => {};
  vi.mocked(pluginsApi.remove).mockImplementation(
    () =>
      new Promise<void>((resolve) => {
        release = resolve;
      }),
  );
  mount();
  await screen.findByRole("heading", { name: "Lyrics A" });
  act(() => {
    reportCredentialRequirement("bob", { stamp: { installationId: "a", installationRevision: 3 } });
    reportCredentialRequirement("bob", { stamp: { installationId: "c", installationRevision: 2 } });
  });
  await settle();
  const baseline = reads();
  fireEvent.click(within(card("Lyrics B")).getByRole("button", { name: "Manage" }));
  fireEvent.click(within(card("Lyrics B")).getByRole("button", { name: "Remove" }));
  const dialog = await screen.findByRole("dialog", { name: "Remove Lyrics B?" });
  fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(pluginsApi.remove).toHaveBeenCalledTimes(1));

  recover(state);
  expect(state.afterReads).toBe(0);
  await act(async () => release());
  await waitFor(() => expect(state.afterReads).toBe(1));
  await settle();
  expect(reads()).toBe(baseline + 1);
  expect(card("Lyrics A").textContent).not.toContain("Credentials need updating");
  expect(card("Lyrics C").textContent).toContain("Credentials need updating");
  expect(screen.queryByRole("heading", { name: "Lyrics B" })).toBeNull();
});
