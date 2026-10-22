import { createUuid } from "./uuid";
import type { Action, SyncBatch, SyncReceipt } from "./types";
import { requestJson } from "./http";
import { log } from "./log";

function deviceId() {
  const saved = localStorage.getItem("hito-device");
  if (saved) return saved;
  const device = createUuid();
  localStorage.setItem("hito-device", device);
  return device;
}

export function prepareChange(action: Action): SyncBatch {
  return {
    device_id: deviceId(),
    operations: [{ id: createUuid(), action }],
  };
}

export async function submitChange(
  account: string,
  batch: SyncBatch,
  signal?: AbortSignal,
): Promise<SyncReceipt> {
  const { id: operation_id, action } = batch.operations[0];
  log.debug("mutation.started", {
    account_id: account,
    operation_id,
    action: action.kind,
  });
  try {
    const receipt: SyncReceipt = await requestJson<SyncReceipt>("/sync", {
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

export async function change(
  account: string,
  action: Action,
  signal?: AbortSignal,
): Promise<SyncReceipt> {
  return submitChange(account, prepareChange(action), signal);
}
