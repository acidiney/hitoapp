import type { Schema } from "../../shared/schema";
import { dateAndTime, relativeTime } from "../../shared/format";

export function sessionClient(session: Schema<"SessionInfo">) {
  return session.client ?? "Unknown browser";
}

export function signedIn(session: Schema<"SessionInfo">) {
  return dateAndTime(new Date(session.created_at * 1000));
}

export function sessionDetail(
  session: Schema<"SessionInfo">,
  now: number = Date.now(),
) {
  return `Active ${relativeTime(session.last_seen_at, now)} · signed in ${signedIn(session)}`;
}
