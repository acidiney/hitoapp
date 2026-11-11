import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { Play } from "lucide-react";
import { time } from "../../../shared/format";
import Artwork from "../../artwork/components/Artwork";
import FallbackCover from "../../../shared/ui/FallbackCover";
import Button from "../../../shared/ui/Button";
import { releaseCreditLabel } from "../../catalog/model/albumCredit";
import type { ArtistHit } from "../model/artistHits";
import type { TopKind } from "../model/ranking";

export type TopResultData =
  | {
      kind: "artist";
      artist: ArtistHit;
      group: Schema<"ArtistIdentityGroup"> | null;
    }
  | { kind: "release"; release: Schema<"Release"> }
  | { kind: "track"; track: Schema<"Recording"> };

export function topResult(
  kind: TopKind | null,
  artists: ArtistHit[],
  groups: Schema<"ArtistIdentityGroup">[],
  releases: Schema<"Release">[],
  tracks: Schema<"Recording">[],
): TopResultData | null {
  if (kind === "artist" && artists[0]) {
    const artist = artists[0];
    const group = groups.find((item) => `group-${item.id}` === artist.key);
    return { kind, artist, group: group ?? null };
  }
  if (kind === "release" && releases[0]) return { kind, release: releases[0] };
  if (kind === "track" && tracks[0]) return { kind, track: tracks[0] };
  return null;
}

function plural(count: number, word: string) {
  return `${count} ${word}${count === 1 ? "" : "s"}`;
}

export default function TopResult({
  result,
  onPlay,
  onRadio,
  radioBusy,
}: {
  result: TopResultData;
  onPlay: () => void;
  onRadio: (group: Schema<"ArtistIdentityGroup">) => void;
  radioBusy: boolean;
}) {
  const play = (
    <Button variant="action" onClick={onPlay}>
      <Play size={16} fill="currentColor" aria-hidden="true" />
      Play
    </Button>
  );
  if (result.kind === "artist") {
    const { artist, group } = result;
    return (
      <div className="top-result">
        <span className="top-result-art" data-shape="round" aria-hidden="true">
          <FallbackCover kind="artist" id={artist.key} title={artist.name} />
        </span>
        <div className="top-result-copy">
          <Link className="top-result-title" href={artist.href}>
            {artist.name}
          </Link>
          <span>
            Artist · {plural(artist.albums, "album")} ·{" "}
            {plural(artist.tracks, "track")} in your library
          </span>
        </div>
        <div className="top-result-actions">
          {play}
          {group && (
            <Button disabled={radioBusy} onClick={() => onRadio(group)}>
              Start radio
            </Button>
          )}
        </div>
      </div>
    );
  }
  if (result.kind === "release") {
    const { release } = result;
    return (
      <div className="top-result">
        <span className="top-result-art" aria-hidden="true">
          <Artwork
            release={release.id}
            fallback={{
              id: release.id,
              title: release.title,
              artist: release.artist,
            }}
          />
        </span>
        <div className="top-result-copy">
          <Link className="top-result-title" href={`/albums/${release.id}`}>
            {release.title}
          </Link>
          <span>
            Album · {releaseCreditLabel(release) ?? "Unknown artist"} ·{" "}
            {plural(release.track_count, "track")}
          </span>
        </div>
        <div className="top-result-actions">
          <Link className="button action" href={`/albums/${release.id}`}>
            Open album
          </Link>
        </div>
      </div>
    );
  }
  const { track } = result;
  return (
    <div className="top-result">
      <span className="top-result-art" aria-hidden="true">
        <Artwork
          release={track.release_id ?? undefined}
          recording={track.id}
          fallback={{
            id: track.release_id ?? track.id,
            title: track.album ?? track.title,
            artist: track.artist,
          }}
        />
      </span>
      <div className="top-result-copy">
        <Link className="top-result-title" href={`/recordings/${track.id}`}>
          {track.title}
        </Link>
        <span>
          Track · {track.artist} · {time(track.duration_ms / 1000)}
        </span>
      </div>
      <div className="top-result-actions">{play}</div>
    </div>
  );
}
