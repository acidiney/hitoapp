import type { Credentials, User } from "../../../shared/types";
import { request, requestJson } from "../../../shared/http";
import { log } from "../../../shared/log";

const part = encodeURIComponent;

export const accountsApi = {
  async list(account: string, signal?: AbortSignal): Promise<User[]> {
    return requestJson<User[]>("/users", {
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
  ): Promise<User> {
    const user: User = await requestJson<User>("/users", {
      action: "accounts.create",
      method: "POST",
      account,
      body: { username, password } satisfies Credentials,
      signal,
    });
    log.info("account.created", { account_id: user.id });
    return user;
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
