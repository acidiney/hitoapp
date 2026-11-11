import { renderHook, waitFor, act } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { usePluginAuthorization } from "../hooks/usePluginAuthorization";
import { pluginsApi } from "../api/pluginsApi";
import { invalidate } from "../../../shared/invalidation";

vi.mock("../api/pluginsApi", () => ({
  pluginsApi: { connectionStatus: vi.fn() },
}));

const plugin = {
  id: "installation",
  service_id: "service",
  revision: 1,
  checked_at: 1,
  credential_set: false,
  manifest: {
    id: "org.hito.fixture",
    name: "Fixture",
    protocol_version: 1,
    capabilities: [],
    authorization: { kind: "oauth2_pkce" },
  },
} as never;

beforeEach(() => vi.mocked(pluginsApi.connectionStatus).mockReset());

test("a disconnect announced from another view refreshes the connection shown here", async () => {
  vi.mocked(pluginsApi.connectionStatus)
    .mockResolvedValueOnce({ status: "connected", scopes: [] })
    .mockResolvedValueOnce({ status: "not_connected", scopes: [] });
  const { result } = renderHook(() =>
    usePluginAuthorization("alice", plugin, false),
  );
  await waitFor(() =>
    expect(result.current.connection?.status).toBe("connected"),
  );
  act(() => invalidate("plugins", "installation"));
  await waitFor(() =>
    expect(result.current.connection?.status).toBe("not_connected"),
  );
});
