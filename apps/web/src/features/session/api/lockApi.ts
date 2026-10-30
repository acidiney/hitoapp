import type { Schema } from "../../../shared/schema";

import { requestJson } from "../../../shared/http";
import { log } from "../../../shared/log";

export const lockApi = {
  state(account: string, signal?: AbortSignal): Promise<Schema<"LockStatus">> {
    return requestJson<Schema<"LockStatus">>("/account/lock", {
      action: "lock.state",
      account,
      signal,
      cache: "no-store",
    });
  },
  activity(
    account: string,
    signal?: AbortSignal,
  ): Promise<Schema<"LockStatus">> {
    return requestJson<Schema<"LockStatus">>("/account/activity", {
      action: "lock.activity",
      method: "POST",
      account,
      signal,
    });
  },
  async lock(account: string): Promise<Schema<"LockStatus">> {
    const status = await requestJson<Schema<"LockStatus">>("/account/lock", {
      action: "lock.lock",
      method: "POST",
      account,
    });
    log.info("lock.locked", { account_id: account });
    return status;
  },
  async unlock(account: string, pin: string): Promise<Schema<"LockStatus">> {
    const status = await requestJson<Schema<"LockStatus">>("/account/unlock", {
      action: "lock.unlock",
      method: "POST",
      account,
      body: { pin },
    });
    log.info("lock.unlocked", { account_id: account });
    return status;
  },
  async setPin(
    account: string,
    request: Schema<"SetPin">,
  ): Promise<Schema<"LockStatus">> {
    const status = await requestJson<Schema<"LockStatus">>("/account/pin", {
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
  async removePin(
    account: string,
    request: Schema<"RemovePin">,
  ): Promise<Schema<"LockStatus">> {
    const status = await requestJson<Schema<"LockStatus">>(
      "/account/pin/remove",
      {
        action: "lock.remove_pin",
        method: "POST",
        account,
        body: request,
      },
    );
    log.info("lock.pin_removed", { account_id: account });
    return status;
  },
};
