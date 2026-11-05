import type { Schema } from "../../../shared/schema";

import { request, requestJson } from "../../../shared/http";
import { log } from "../../../shared/log";

export const accountApi = {
  async changePassword(
    account: string,
    body: Schema<"ChangePassword">,
    signal?: AbortSignal,
  ): Promise<Schema<"SessionsRevoked">> {
    const result = await requestJson<Schema<"SessionsRevoked">>(
      "/account/password",
      {
        action: "account.change_password",
        method: "POST",
        account,
        body,
        signal,
      },
    );
    log.info("account.password_changed", {
      account_id: account,
      count: result.revoked,
    });
    return result;
  },
  async setDisplayName(
    account: string,
    displayName: string | null,
    signal?: AbortSignal,
  ): Promise<Schema<"User">> {
    const body: Schema<"UpdateDisplayName"> = { display_name: displayName };
    const user = await requestJson<Schema<"User">>("/account/display-name", {
      action: "account.display_name",
      method: "PUT",
      account,
      body,
      signal,
    });
    log.info("account.display_name_changed", {
      account_id: account,
      count: user.display_name ? 1 : 0,
    });
    return user;
  },
  async setAvatar(
    account: string,
    file: Blob,
    signal?: AbortSignal,
  ): Promise<Schema<"User">> {
    const user = await requestJson<Schema<"User">>("/account/avatar", {
      action: "account.avatar_changed",
      method: "PUT",
      account,
      file,
      signal,
    });
    log.info("account.avatar_changed", { account_id: account });
    return user;
  },
  async removeAvatar(
    account: string,
    signal?: AbortSignal,
  ): Promise<Schema<"User">> {
    const user = await requestJson<Schema<"User">>("/account/avatar", {
      action: "account.avatar_removed",
      method: "DELETE",
      account,
      signal,
    });
    log.info("account.avatar_removed", { account_id: account });
    return user;
  },
  sessions(
    account: string,
    signal?: AbortSignal,
  ): Promise<Schema<"SessionInfo">[]> {
    return requestJson<Schema<"SessionInfo">[]>("/account/sessions", {
      action: "account.sessions",
      account,
      signal,
      cache: "no-store",
    });
  },
  async revokeSession(
    account: string,
    id: string,
    signal?: AbortSignal,
  ): Promise<void> {
    await request(`/account/sessions/${encodeURIComponent(id)}`, {
      action: "account.revoke_session",
      method: "DELETE",
      account,
      signal,
      errorBody: false,
    });
    log.info("account.session_revoked", { account_id: account });
  },
  async revokeOthers(
    account: string,
    signal?: AbortSignal,
  ): Promise<Schema<"SessionsRevoked">> {
    const result = await requestJson<Schema<"SessionsRevoked">>(
      "/account/sessions",
      {
        action: "account.revoke_other_sessions",
        method: "DELETE",
        account,
        signal,
      },
    );
    log.info("account.other_sessions_revoked", {
      account_id: account,
      count: result.revoked,
    });
    return result;
  },
};
