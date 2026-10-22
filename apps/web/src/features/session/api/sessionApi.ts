import type { Credentials } from "../../../shared/types";
import { request } from "../../../shared/http";
import { log } from "../../../shared/log";
import { notifySessionChanged } from "../session";

export const sessionApi = {
  async signIn(
    setup: boolean,
    username: string,
    password: string,
  ): Promise<void> {
    const action = setup ? "session.setup" : "session.login";
    log.info("session.started", { action });
    try {
      await request(setup ? "/setup" : "/login", {
        action,
        method: "POST",
        body: { username, password } satisfies Credentials,
      });
      notifySessionChanged();
      log.info("session.accepted", { action });
    } catch (error) {
      log.warn("session.failed", { action });
      throw error;
    }
  },
  async signOut(account: string): Promise<void> {
    await request("/logout", {
      action: "session.logout",
      method: "POST",
      account,
    });
    notifySessionChanged();
    log.info("session.ended", { account_id: account });
  },
};
