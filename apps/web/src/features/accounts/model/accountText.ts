import type { Schema } from "../../../shared/schema";
import { relativeTime } from "../../../shared/format";

export function accountHint(
  account: Schema<"AccountSummary">,
  now: number = Date.now(),
) {
  const role = account.admin ? "Administrator" : "Member";
  const activity =
    account.last_active_at == null
      ? "No open sessions"
      : `Last active ${relativeTime(account.last_active_at, now)}`;
  const identity = account.display_name ? `${account.username} · ` : "";
  return `${identity}${role} · ${activity}`;
}
