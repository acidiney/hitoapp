import type { LockStatus, RemovePin, SetPin } from "../../../shared/types";
import { requestJson } from "../../../shared/http";
import { log } from "../../../shared/log";

export const lockApi = {
  state(account: string, signal?: AbortSignal): Promise<LockStatus> {
    return requestJson<LockStatus>("/account/lock", {
      action: "lock.state",
      account,
      signal,
      cache: "no-store",
    });
  },
  activity(account: string, signal?: AbortSignal): Promise<LockStatus> {
    return requestJson<LockStatus>("/account/activity", {
      action: "lock.activity",
      method: "POST",
      account,
      signal,
    });
  },
  async lock(account: string): Promise<LockStatus> {
    const status = await requestJson<LockStatus>("/account/lock", {
      action: "lock.lock",
      method: "POST",
      account,
    });
    log.info("lock.locked", { account_id: account });
    return status;
  },
  async unlock(account: string, pin: string): Promise<LockStatus> {
    const status = await requestJson<LockStatus>("/account/unlock", {
      action: "lock.unlock",
      method: "POST",
      account,
      body: { pin },
    });
    log.info("lock.unlocked", { account_id: account });
    return status;
  },
  async setPin(account: string, request: SetPin): Promise<LockStatus> {
    const status = await requestJson<LockStatus>("/account/pin", {
      action: "lock.set_pin",
      method: "PUT",
      account,
      body: request,
    });
    log.info("lock.pin_saved", {
      account_id: account,
      enabled: status.pin_enabled,
    });
    return status;
  },
  async removePin(account: string, request: RemovePin): Promise<LockStatus> {
    const status = await requestJson<LockStatus>("/account/pin/remove", {
      action: "lock.remove_pin",
      method: "POST",
      account,
      body: request,
    });
    log.info("lock.pin_removed", { account_id: account });
    return status;
  },
};
