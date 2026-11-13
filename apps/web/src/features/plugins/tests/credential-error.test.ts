import type { Schema } from "../../../shared/schema";
import { afterEach, expect, test, vi } from "vitest";
import { HttpError, request } from "../../../shared/http";
import {
  credentialRequirement,
  healthNeedsCredentials,
} from "../model/credentialError";

afterEach(() => vi.unstubAllGlobals());

async function failure(status: number, body: unknown) {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(Response.json(body, { status })),
  );
  return request("/recordings/r/lyrics/identities/read", {
    action: "lyrics.read_selected",
    method: "POST",
    body: {},
  }).catch((error: unknown) => error);
}

const stamp = {
  installation_id: "inst-1",
  installation_revision: 4,
} satisfies Schema<"PluginInstallationStamp">;
const credentialBody = {
  error: "This music service connection needs its credentials updated.",
  code: "plugin_credentials_required",
  plugin_connection: stamp,
} satisfies Schema<"ErrorResponse">;

test("the credential 409 is recognised with its connection stamp", async () => {
  const error = await failure(409, credentialBody);
  expect(error).toBeInstanceOf(HttpError);
  expect(credentialRequirement(error)).toEqual({
    stamp: { installationId: "inst-1", installationRevision: 4 },
  });
});

test("the credential 409 without a stamp is recognised without any connection", async () => {
  const bare = { ...credentialBody, plugin_connection: undefined };
  expect(credentialRequirement(await failure(409, bare))).toEqual({
    stamp: null,
  });
});

test.each([
  ["a missing revision", { installation_id: "inst-1" }],
  [
    "a text revision",
    { installation_id: "inst-1", installation_revision: "4" },
  ],
  [
    "a fractional revision",
    { installation_id: "i", installation_revision: 1.5 },
  ],
  ["a negative revision", { installation_id: "i", installation_revision: -1 }],
  ["an empty installation", { installation_id: "", installation_revision: 1 }],
  ["a numeric installation", { installation_id: 7, installation_revision: 1 }],
  ["a stamp that is not an object", "inst-1"],
])("a malformed stamp (%s) is treated as absent", async (_name, bad) => {
  const error = await failure(409, {
    ...credentialBody,
    plugin_connection: bad,
  });
  expect(credentialRequirement(error)).toEqual({ stamp: null });
});

test("a listener 401, an inaccessible 404 and an unrelated 409 are not credential errors even with the same code or stamp", async () => {
  expect(credentialRequirement(await failure(401, credentialBody))).toBeNull();
  expect(credentialRequirement(await failure(404, credentialBody))).toBeNull();
  expect(
    credentialRequirement(
      await failure(409, { error: "Changed", code: "stale", ...stamp }),
    ),
  ).toBeNull();
  expect(
    credentialRequirement(await failure(409, { error: "Changed" })),
  ).toBeNull();
  expect(credentialRequirement(new Error("network"))).toBeNull();
  expect(credentialRequirement(null)).toBeNull();
});

test("only the credentials health value needs credentials and a missing value is unknown", () => {
  const health = (state: Schema<"PluginHealthState">) =>
    ({ state }) satisfies Schema<"PluginHealth">;
  expect(healthNeedsCredentials(health("credentials_required"))).toBe(true);
  expect(healthNeedsCredentials(health("ok"))).toBe(false);
  expect(healthNeedsCredentials(health("degraded"))).toBe(false);
  expect(healthNeedsCredentials(undefined)).toBe(false);
  expect(healthNeedsCredentials(null)).toBe(false);
});
