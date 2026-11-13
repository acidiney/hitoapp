import { createElement, StrictMode, type ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { HttpError } from "../../../shared/http";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import {
  credentialMarkVersion,
  reportCredentialRequirement,
  resetCredentialRequirements,
} from "../../plugins/model/credentialRequirements";
import { useSourceAttachment } from "../hooks/useSourceAttachment";

vi.mock("../../plugins/api/pluginsApi", () => ({
  pluginsApi: { settings: vi.fn(), search: vi.fn() },
}));
vi.mock("../api/sourceBindingsApi", () => ({
  sourceBindingsApi: { review: vi.fn(), attach: vi.fn() },
}));

const strict = ({ children }: { children: ReactNode }) =>
  createElement(StrictMode, null, children);

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    services: [
      {
        id: "service",
        name: "Provider",
        base_url: "https://p.example/",
        revision: 1,
      },
    ],
    installations: [
      {
        id: "a",
        service_id: "service",
        revision: 3,
        checked_at: 1,
        credential_set: true,
        manifest: {
          id: "org.hito.a",
          name: "Provider A",
          protocol_version: 1,
          capabilities: ["search", "sources"],
        },
      },
    ],
    credential_storage_ready: true,
  });
});

afterEach(() => resetCredentialRequirements());

test("a stamped credential rejection from source search is reported once even when React replays the state updater", async () => {
  vi.mocked(pluginsApi.search).mockRejectedValue(
    new HttpError(
      409,
      "Credentials required",
      "plugin_credentials_required",
      undefined,
      undefined,
      { plugin_connection: { installation_id: "a", installation_revision: 3 } },
    ),
  );
  reportCredentialRequirement("bob", {
    stamp: { installationId: "baseline", installationRevision: 1 },
  });
  const baseline = credentialMarkVersion("bob", "baseline")!;
  const { result } = renderHook(
    () => useSourceAttachment("bob", "recording", () => {}),
    { wrapper: strict },
  );
  await act(async () => {
    await result.current.open();
  });
  await waitFor(() => expect(result.current.connection?.id).toBe("a"));
  await act(async () => {
    await result.current.search("owned query");
  });
  await waitFor(() => expect(result.current.error).not.toBe(""));
  expect(pluginsApi.search).toHaveBeenCalledTimes(1);
  expect(credentialMarkVersion("bob", "a")).toBe(baseline + 1);
});
