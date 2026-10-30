import type { Schema } from "../../../shared/schema";

import { request, requestJson } from "../../../shared/http";
import { log } from "../../../shared/log";

const part = encodeURIComponent;

export const accountsApi = {
  async list(account: string, signal?: AbortSignal): Promise<Schema<"User">[]> {
    return requestJson<Schema<"User">[]>("/users", {
      action: "accounts.list",
      account,
      signal,
    });
  },
  async create(
    account: string,
    username: string,
    password: string,
    signal?: AbortSignal,
  ): Promise<Schema<"User">> {
    const user: Schema<"User"> = await requestJson<Schema<"User">>("/users", {
      action: "accounts.create",
      method: "POST",
      account,
      body: { username, password } satisfies Schema<"Credentials">,
      signal,
    });
    log.info("account.created", { account_id: user.id });
    return user;
  },
  async remove(
    account: string,
    user: Schema<"User">,
    signal?: AbortSignal,
  ): Promise<void> {
    await request(`/users/${part(user.id)}`, {
      action: "accounts.delete",
      method: "DELETE",
      account,
      body: { username: user.username } satisfies Schema<"DeleteAccount">,
      signal,
    });
    log.info("account.deleted", {});
  },
  async setLibraryAccess(
    account: string,
    library: string,
    user: string,
    allowed: boolean,
    signal?: AbortSignal,
  ): Promise<void> {
    await request(`/libraries/${part(library)}/members/${part(user)}`, {
      action: "accounts.library_access",
      method: allowed ? "PUT" : "DELETE",
      account,
      signal,
    });
    log.info("account.access_changed", { account_id: user, enabled: allowed });
  },
};
