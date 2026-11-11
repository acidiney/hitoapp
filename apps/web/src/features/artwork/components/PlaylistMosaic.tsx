import Artwork from "./Artwork";

export type MosaicCover = {
  key: string;
  release?: string;
  recording: string;
  title: string;
};

export default function PlaylistMosaic({ covers }: { covers: MosaicCover[] }) {
  return (
    <div className="artwork playlist-mosaic" aria-hidden="true">
      {covers.slice(0, 4).map((cover) => (
        <Artwork
          key={cover.key}
          release={cover.release}
          recording={cover.recording}
          fallback={{
            id: cover.release ?? cover.recording,
            title: cover.title,
            kind: "album",
            inline: false,
          }}
        />
      ))}
    </div>
  );
}

export function mosaicCovers(
  tracks: {
    id: string;
    release_id?: string | null;
    album?: string | null;
    title: string;
  }[],
): MosaicCover[] {
  const seen = new Set<string>();
  const covers: MosaicCover[] = [];
  for (const track of tracks) {
    const key = track.release_id ?? track.id;
    if (seen.has(key)) continue;
    seen.add(key);
    covers.push({
      key,
      release: track.release_id ?? undefined,
      recording: track.id,
      title: track.album ?? track.title,
    });
    if (covers.length === 4) break;
  }
  return covers;
}
