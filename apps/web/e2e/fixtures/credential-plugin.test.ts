import type { APIRequestContext } from "@playwright/test";
import { expect, test, vi } from "vitest";
import { connectCredentialPlugin } from "./credential-plugin";

const service = {
  id: "service",
  name: "Owned fake",
  base_url: "http://127.0.0.1:1/",
  revision: 1,
};

test("a failed connection setup keeps its own error even when both cleanup steps also fail", async () => {
  const lookup = vi.fn(async () => {
    throw new Error("settings lookup failed during cleanup");
  });
  const administrator = {
    post: vi.fn(async () => ({
      status: () => 201,
      json: async () => service,
    })),
    get: lookup,
  } as unknown as APIRequestContext;
  const request = {
    post: vi.fn(async () => ({ status: () => 500 })),
  } as unknown as APIRequestContext;
  const close = vi.fn(async () => {
    throw new Error("fake plugin close failed");
  });
  await expect(
    connectCredentialPlugin(administrator, request, "http://localhost", {
      label: "Owned fake",
      address: service.base_url,
      initialToken: "owned-token",
      close,
    }),
  ).rejects.toThrow("The connection was refused");
  expect(lookup).toHaveBeenCalledTimes(1);
  expect(close).toHaveBeenCalledTimes(1);
});
