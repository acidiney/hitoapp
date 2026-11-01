import type { Schema } from "../../../shared/schema";
import { newCapabilities } from "../../plugins/model/pluginSummary";

type Review = Schema<"PlaylistFollowReview">;
type Change = Schema<"PlaylistFollowChange">;
type Capability = Schema<"Manifest">["capabilities"][number];

const FIELD_LABELS: Record<Change["kind"], string> = {
  capabilities: "Abilities",
  credential: "Access token",
  removed: "Connection",
};

function joined(items: string[]) {
  return items.length < 2
    ? items.join("")
    : `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

export function changeLabel(change: Change) {
  return FIELD_LABELS[change.kind];
}

export function changeValue(values: string[]) {
  return values.length === 0
    ? "None"
    : values
        .map((value) => value.replaceAll("_", " "))
        .sort()
        .join(", ");
}

export function reviewSentence(review: Review) {
  const name = review.connection_name;
  const generic = `The ${name} connection changed.`;
  if (review.changes.length !== 1) return generic;
  const [change] = review.changes;
  if (change.kind === "removed")
    return `The ${name} connection was removed or replaced.`;
  if (change.kind === "credential")
    return `The saved access token for ${name} changed.`;
  if (!change.after.includes("playlists"))
    return `The ${name} connection no longer offers playlists.`;
  const gained = newCapabilities(
    change.before as Capability[],
    change.after as Capability[],
  );
  return gained.length > 0
    ? `The ${name} connection now also offers ${joined(gained)}.`
    : generic;
}
