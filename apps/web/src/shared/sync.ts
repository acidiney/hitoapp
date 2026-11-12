import type { Schema } from "./schema";
import { createUuid } from "./uuid";

import { requestJson } from "./http";
import { log } from "./log";
import { invalidate, invalidationsFor } from "./invalidation";

function deviceId() {
  const saved = localStorage.getItem("hito-device");
  if (saved) return saved;
  const device = createUuid();
  localStorage.setItem("hito-device", device);
  return device;
}

export function prepareChange(action: Schema<"Action">): Schema<"SyncBatch"> {
  return {
    device_id: deviceId(),
    operations: [{ id: createUuid(), action }],
  };
}

export async function submitChange(
  account: string,
  batch: Schema<"SyncBatch">,
  signal?: AbortSignal,
): Promise<Schema<"SyncReceipt">> {
  const { id: operation_id, action } = batch.operations[0];
  log.debug("mutation.started", {
    account_id: account,
    operation_id,
    action: action.kind,
  });
  try {
    const receipt: Schema<"SyncReceipt"> = await requestJson<
      Schema<"SyncReceipt">
    >("/sync", {
      action: action.kind === "progress_set" ? "progress.save" : "sync.push",
      method: "POST",
      body: batch,
      account,
      signal,
    });
    const acknowledged = receipt?.acknowledged;
    if (
      !Array.isArray(acknowledged) ||
      !acknowledged.every((id: unknown) => typeof id === "string") ||
      !batch.operations.every((operation) =>
        acknowledged.includes(operation.id),
      )
    )
      throw new Error(
        "Hito did not acknowledge this change. Retry the same request.",
      );
    const accepted = action.kind === "progress_set" ? log.debug : log.info;
    accepted("mutation.accepted", {
      account_id: account,
      operation_id,
      action: action.kind,
    });
    for (const operation of batch.operations)
      for (const event of invalidationsFor(operation.action))
        invalidate(event.scope, event.id, event.change);
    return receipt;
  } catch (error) {
    if (!signal?.aborted)
      log.warn("mutation.failed", {
        account_id: account,
        operation_id,
        action: action.kind,
      });
    throw error;
  }
}

export function prepareChanges(
  actions: Schema<"Action">[],
): Schema<"SyncBatch"> {
  return {
    device_id: deviceId(),
    operations: actions.map((action) => ({ id: createUuid(), action })),
  };
}

export async function changeMany(
  account: string,
  actions: Schema<"Action">[],
  signal?: AbortSignal,
): Promise<Schema<"SyncReceipt">> {
  return submitChange(account, prepareChanges(actions), signal);
}

export async function change(
  account: string,
  action: Schema<"Action">,
  signal?: AbortSignal,
): Promise<Schema<"SyncReceipt">> {
  return submitChange(account, prepareChange(action), signal);
}
