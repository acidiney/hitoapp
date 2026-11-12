import type { Schema } from "../../../shared/schema";
import { fireEvent, screen } from "@testing-library/react";
import { expect, vi } from "vitest";

export type Persistence = NonNullable<
  Schema<"PluginInstallation">["manifest"]["content_policy"]
>["source_persistence"];

export const installationAccount = "install-policy-listener";
export const installationDigest = "private-install-preview-digest-sentinel";
export const installationToken = "private-install-token-sentinel";
export const replacementInstallationToken =
  "private-install-replacement-token-sentinel";
export const replacementInstallationDigest =
  "private-install-replacement-digest-sentinel";
export const offeredService: Schema<"PluginService"> = {
  id: "install-policy-service",
  name: "private-install-offer-name-sentinel",
  base_url: "https://private-install.invalid/private-path/",
  revision: 3,
};
export const otherService: Schema<"PluginService"> = {
  ...offeredService,
  id: "other-install-service",
  name: "Another service",
};

export function installationManifest(policy?: Persistence) {
  return {
    id: "org.hito.private-install-manifest-sentinel",
    name: "private-install-manifest-name-sentinel",
    protocol_version: 1,
    capabilities: ["search", "sources"],
    ...(policy ? { content_policy: { source_persistence: policy } } : {}),
  } satisfies Schema<"PluginInstallation">["manifest"];
}

export function confirmedInstallation(
  policy?: Persistence,
): Schema<"PluginInstallation"> {
  return {
    id: "install-policy-installation",
    service_id: offeredService.id,
    revision: 1,
    checked_at: 1,
    credential_set: true,
    manifest: installationManifest(policy),
  };
}

type InstallationRequest = {
  path: string;
  method: string;
  body?: unknown;
};
export type InstallationHttpCall = InstallationRequest & {
  account: string | null;
  signal: AbortSignal | null | undefined;
};

export function installationHttp(policy?: Persistence) {
  const requests: InstallationRequest[] = [];
  const calls: InstallationHttpCall[] = [];
  const checks: string[] = [];
  const state: {
    services: Schema<"PluginService">[];
    installations: Schema<"PluginInstallation">[];
    preview: Schema<"PluginInstallationPreview">;
    respond?: (
      call: InstallationHttpCall,
    ) => Response | Promise<Response> | undefined;
  } = {
    services: [offeredService, otherService],
    installations: [],
    preview: {
      manifest: installationManifest(policy),
      digest: installationDigest,
    },
  };
  const accounts = new Set([installationAccount]);
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      const path = String(input);
      const method = options?.method ?? "GET";
      const body: unknown = options?.body
        ? JSON.parse(String(options.body))
        : undefined;
      if (path.endsWith("/declaration-check")) {
        checks.push(path);
        return Response.json({ kind: "current" });
      }
      const request = { path, method, body };
      requests.push(request);
      const account = new Headers(options?.headers).get("x-hito-account");
      expect(account).not.toBeNull();
      expect(accounts.has(account!)).toBe(true);
      const call = { ...request, account, signal: options?.signal };
      calls.push(call);
      const response = state.respond?.(call);
      if (response) return response;
      if (path === "/api/v1/plugins" && method === "GET")
        return Response.json({
          credential_storage_ready: true,
          services: state.services,
          installations: state.installations,
        });
      if (path === "/api/v1/plugins/preview" && method === "POST")
        return Response.json(state.preview);
      if (path === "/api/v1/plugins" && method === "POST")
        return Response.json({
          ...confirmedInstallation(policy),
          manifest: state.preview.manifest,
        });
      return Response.json({ error: "Unexpected request" }, { status: 500 });
    }),
  );
  return { requests, calls, checks, state, accounts };
}

export function heldInstallationResponse(
  mode: "headers" | "body",
  value: unknown,
) {
  let release!: () => void;
  if (mode === "headers") {
    const response = new Promise<Response>((resolve) => {
      release = () => resolve(Response.json(value));
    });
    return { response, release: () => release() };
  }
  const response = Response.json(value);
  const body = new Promise<unknown>((resolve) => {
    release = () => resolve(value);
  });
  response.json = () => body;
  return { response, release: () => release() };
}

export async function openManage() {
  const buttons = await screen.findAllByRole("button", { name: "Manage" });
  for (const button of buttons)
    if (button.getAttribute("aria-expanded") === "false")
      fireEvent.click(button);
}

export async function findCheckConnection() {
  await openManage();
  return screen.findByRole("button", { name: "Check connection" });
}
