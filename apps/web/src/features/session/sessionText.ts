import type { Schema } from "../../shared/schema";
import { relativeTime } from "../../shared/format";

export function sessionClient(session: Schema<"SessionInfo">) {
  return session.client ?? "Unknown browser";
}

export function sessionActivity(
  session: Schema<"SessionInfo">,
  now: number = Date.now(),
) {
  return `${sessionClient(session)} · Last active ${relativeTime(session.last_seen_at, now)}`;
}

export function signedIn(session: Schema<"SessionInfo">) {
  return new Date(session.created_at * 1000).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}
