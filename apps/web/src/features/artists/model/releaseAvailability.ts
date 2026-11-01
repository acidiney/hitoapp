import type { Schema } from "../../../shared/schema";

export type ReleaseAvailability = {
  state: "library" | "catalog";
  label?: string;
  complete: boolean;
};

export function releaseAvailability(
  albums: Schema<"LibraryAlbum">[],
): ReleaseAvailability {
  if (albums.length === 0) return { state: "catalog", complete: false };
  const best = albums.reduce((a, b) =>
    b.playable_count > a.playable_count ? b : a,
  );
  if (best.track_count > 0 && best.playable_count === best.track_count)
    return { state: "library", complete: true };
  return {
    state: "library",
    label: `Partial · ${best.playable_count} of ${best.track_count}`,
    complete: false,
  };
}
