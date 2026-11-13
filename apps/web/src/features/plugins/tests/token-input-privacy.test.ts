import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
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
import { resetCredentialRequirements } from "../model/credentialRequirements";

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

const secret = "typed-bearer-secret-value";

function expectNoSecret(field: HTMLInputElement) {
  expect(field.hasAttribute("value")).toBe(false);
  expect(document.documentElement.outerHTML).not.toContain(secret);
  expect(document.body.textContent).not.toContain(secret);
}

test("a typed bearer token never becomes a value attribute or page text while entering, saving or after saving", async () => {
  let finish: (value: Awaited<ReturnType<typeof pluginsApi.refresh>>) => void =
    () => {};
  vi.mocked(pluginsApi.refresh).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  show(installation("a", "Lyrics A", 3));
  await screen.findByRole("heading", { name: "Lyrics A" });
  fireEvent.click(within(card("Lyrics A")).getByRole("button", { name: "Manage" }));
  const field = screen.getByLabelText<HTMLInputElement>("New bearer token");
  fireEvent.change(field, { target: { value: secret } });
  expect(field.value).toBe(secret);
  expectNoSecret(field);

  fireEvent.click(screen.getByRole("button", { name: "Save token" }));
  await waitFor(() => expect(pluginsApi.refresh).toHaveBeenCalledTimes(1));
  expect(vi.mocked(pluginsApi.refresh).mock.calls[0][2]).toMatchObject({
    credential: { action: "replace", bearer_token: secret },
  });
  expectNoSecret(field);

  await act(async () =>
    finish({ kind: "updated", installation: installation("a", "Lyrics A", 4) }),
  );
  await screen.findByText("Connection checked.");
  expect(screen.getByLabelText<HTMLInputElement>("New bearer token").value).toBe("");
  expectNoSecret(screen.getByLabelText<HTMLInputElement>("New bearer token"));
});

test("a failed save keeps the typed token only in the field property, never in an attribute", async () => {
  vi.mocked(pluginsApi.refresh).mockRejectedValue(new Error("Save failed"));
  show(installation("a", "Lyrics A", 3));
  await screen.findByRole("heading", { name: "Lyrics A" });
  fireEvent.click(within(card("Lyrics A")).getByRole("button", { name: "Manage" }));
  const field = screen.getByLabelText<HTMLInputElement>("New bearer token");
  fireEvent.change(field, { target: { value: secret } });
  fireEvent.click(screen.getByRole("button", { name: "Save token" }));
  await waitFor(() => expect(pluginsApi.refresh).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(document.body.textContent).toContain("Save failed"));
  expectNoSecret(field);
});
