import ActionMenu, { type ActionMenuItem } from "../../../shared/ui/ActionMenu";
import { useArtwork } from "./ArtworkProvider";

export type ArtworkSubject =
  | { recording: string; release?: never; playlistId?: never }
  | { recording?: never; release: string; playlistId?: never }
  | { recording?: never; release?: never; playlistId: string };

export function useArtworkRetryItem(
  subject: ArtworkSubject,
): ActionMenuItem | null {
  const { recording, release, playlistId } = subject;
  const artwork = useArtwork(
    playlistId ? "playlist" : release ? "release" : "recording",
    playlistId ?? release ?? recording,
  );
  if (!artwork.imageFailed || artwork.phase !== "unavailable") return null;
  return { label: "Retry artwork", onSelect: artwork.retry };
}

export default function ArtworkRetryMenu(subject: ArtworkSubject) {
  const retry = useArtworkRetryItem(subject);
  if (!retry) return null;
  return <ActionMenu label="More options" items={[retry]} />;
}
