import { beforeEach, expect, test, vi } from "vitest";
import { prepareChange, submitChange } from "./api";
import type { Action, SyncBatch, SyncReceipt } from "./types";

const action: Action = {
  kind: "playlist_follow_resume",
  playlist_id: "9fc57926-55f4-47ca-81e0-e8ca89c889cd",
  expected_generation: 3,
  installation_revision: 5,
  service_revision: 2,
};
const account = "3d4ed601-4196-4f66-9a3e-a36231e6785e";
const uuid =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

beforeEach(() => {
  localStorage.clear();
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
});

test("a prepared Resume retries the same device, operation and action after a lost response", async () => {
  const fetch = vi.fn<typeof globalThis.fetch>();
  vi.stubGlobal("fetch", fetch);
  const batch: SyncBatch = prepareChange(action);
  expect(fetch).not.toHaveBeenCalled();
  expect(batch.device_id).toMatch(uuid);
  expect(localStorage.getItem("hito-device")).toBe(batch.device_id);
  expect(batch.operations).toEqual([
    { id: expect.stringMatching(uuid), action },
  ]);
  expect(batch.operations[0].id).not.toBe(batch.device_id);
  const prepared = JSON.stringify(batch);
  const receipt: SyncReceipt = { acknowledged: [batch.operations[0].id] };
  const lost = new TypeError("Owned sync fixture lost its response");
  fetch
    .mockRejectedValueOnce(lost)
    .mockResolvedValueOnce(Response.json(receipt));
  const first = new AbortController();
  const retry = new AbortController();

  await expect(submitChange(account, batch, first.signal)).rejects.toBe(lost);
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(JSON.stringify(batch)).toBe(prepared);
  await expect(submitChange(account, batch, retry.signal)).resolves.toEqual(
    receipt,
  );
  expect(fetch).toHaveBeenCalledTimes(2);
  for (const [path, options] of fetch.mock.calls) {
    expect(path).toBe("/api/v1/sync");
    expect(options).toMatchObject({
      method: "POST",
      credentials: "same-origin",
      headers: {
        "content-type": "application/json",
        "x-hito-request": "1",
        "x-hito-account": account,
      },
      body: prepared,
    });
  }
  expect(fetch.mock.calls[0][1]?.signal).toBe(first.signal);
  expect(fetch.mock.calls[1][1]?.signal).toBe(retry.signal);
  expect(JSON.stringify(batch)).toBe(prepared);
  expect(localStorage.getItem("hito-device")).toBe(batch.device_id);
});

test.each<SyncReceipt>([
  { acknowledged: [] },
  { acknowledged: ["f54d4874-0039-4992-874a-fc26e4dd911b"] },
])(
  "a successful HTTP response must acknowledge the prepared operation: %j",
  async (receipt) => {
    const savedDevice = "aa4f51b9-955c-408a-ae8e-fe1f340af07d";
    localStorage.setItem("hito-device", savedDevice);
    const fetch = vi
      .fn<typeof globalThis.fetch>()
      .mockResolvedValue(Response.json(receipt));
    vi.stubGlobal("fetch", fetch);
    const batch: SyncBatch = prepareChange(action);
    expect(batch.device_id).toBe(savedDevice);
    const prepared = JSON.stringify(batch);

    await expect(submitChange(account, batch)).rejects.toThrow();

    expect(fetch).toHaveBeenCalledExactlyOnceWith(
      "/api/v1/sync",
      expect.objectContaining({
        method: "POST",
        credentials: "same-origin",
        headers: expect.objectContaining({ "x-hito-account": account }),
        body: prepared,
      }),
    );
    expect(JSON.stringify(batch)).toBe(prepared);
    expect(localStorage.getItem("hito-device")).toBe(savedDevice);
  },
);
