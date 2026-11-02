import { request } from "../../../shared/http";

export type CoverArtKind =
  | "release"
  | "release-group"
  | "artist"
  | "credited-artist";

export function coverArtUrl(kind: CoverArtKind, id: string): string {
  return `/api/v1/cover-art/${kind}/${encodeURIComponent(id)}`;
}

export async function probeArtistPhoto(
  account: string,
  id: string,
  signal: AbortSignal,
): Promise<void> {
  await request(`/cover-art/artist/${encodeURIComponent(id)}`, {
    action: "artist_group.photo_probe",
    account,
    signal,
    errorBody: false,
  });
}
