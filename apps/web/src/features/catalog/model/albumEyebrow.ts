import type { Schema } from "../../../shared/schema";

export const releaseKinds: Record<Schema<"CatalogReleaseGroupKind">, string> = {
  album: "Album",
  ep: "EP",
  single: "Single",
  broadcast: "Broadcast",
  other: "Other",
};

export function albumEyebrow(
  kind: Schema<"CatalogReleaseGroupKind"> | null | undefined,
  year: string | number | null | undefined,
): string {
  return [kind ? releaseKinds[kind] : "Album", year]
    .filter(Boolean)
    .join(" · ");
}
