import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { useReviewedArtistProfile } from "../../artists/hooks/useReviewedArtistProfile";
import { useReleaseSuggestion } from "../hooks/useReleaseSuggestion";
import AlbumEditionStep, { type AlbumFacts } from "./AlbumEditionStep";
import MatchPanel from "./MatchPanel";
import ReleasePicker from "./ReleasePicker";

type Props = {
  account: string;
  release: string;
  anchor: Schema<"AlbumCatalogAnchor">;
  album: AlbumFacts;
  remembered: boolean;
  remember: (edition: Schema<"SavedAlbumEdition"> | null) => void;
  dismiss: () => void;
  forget?: () => void;
  busy?: boolean;
};

export default function AlbumReleaseMatch(props: Props) {
  const { account, anchor, album, dismiss } = props;
  const artist = useReviewedArtistProfile(account, anchor);
  if (artist.error)
    return (
      <MatchPanel title="Showing only your tracks">
        <p role="alert" className="match-alert">
          {artist.error}
        </p>
        <div className="match-actions">
          <button
            type="button"
            className="text-action"
            disabled={artist.pending}
            onClick={() => void artist.retry()}
          >
            Try again
          </button>
          <button type="button" className="text-action" onClick={dismiss}>
            Just show my tracks
          </button>
        </div>
      </MatchPanel>
    );
  if (!artist.ready)
    return (
      <MatchPanel title="Finding the release of this album" busy>
        <p role="status">Looking for this album's release…</p>
      </MatchPanel>
    );
  return <ReleaseMatch {...props} artist={artist.ready} />;
}

function ReleaseMatch({
  account,
  release,
  anchor,
  album,
  remembered,
  remember,
  dismiss,
  forget,
  busy,
  artist,
}: Props & {
  artist: NonNullable<ReturnType<typeof useReviewedArtistProfile>["ready"]>;
}) {
  const suggestion = useReleaseSuggestion(
    account,
    anchor.recording_id,
    artist.association,
    artist.profile,
    album.title,
    album.year,
  );
  const [picked, setPicked] = useState<Schema<"CatalogReleaseGroup"> | null>(
    null,
  );
  const [choosing, setChoosing] = useState(false);
  const group = picked ?? (choosing ? null : suggestion.suggestion);
  const opens = () => setChoosing(true);

  if (group)
    return (
      <AlbumEditionStep
        key={group.id}
        context={{
          account,
          recording: anchor.recording_id,
          association: artist.association,
          providerArtistId: artist.profile.artist.id,
          releaseGroupId: group.id,
          release,
        }}
        group={group}
        album={album}
        suggested={picked === null}
        automatic={picked === null && suggestion.unique}
        remembered={remembered}
        remember={remember}
        forget={forget}
        busy={busy}
        different={() => {
          setPicked(null);
          opens();
        }}
        dismiss={dismiss}
      />
    );
  if (suggestion.failed)
    return (
      <MatchPanel title="Showing only your tracks">
        <p role="alert" className="match-alert">
          {suggestion.error} Your {album.trackCount}{" "}
          {album.trackCount === 1 ? "track is" : "tracks are"} listed and play
          as usual. The full track list needs an edition.
        </p>
        <div className="match-actions">
          <button
            type="button"
            className="text-action"
            disabled={suggestion.pending}
            onClick={() => void suggestion.retry()}
          >
            Try again
          </button>
          <button type="button" className="text-action" onClick={dismiss}>
            Just show my tracks
          </button>
        </div>
      </MatchPanel>
    );
  if (!suggestion.settled)
    return (
      <MatchPanel title="Finding the release of this album" busy>
        <p role="status">Looking for this album in the artist's catalog…</p>
      </MatchPanel>
    );
  if (suggestion.groups.length === 0)
    return (
      <MatchPanel title="No editions listed for this album">
        <p className="match-alert">
          The catalog lists no release for this artist. Your {album.trackCount}{" "}
          {album.trackCount === 1 ? "track is" : "tracks are"} listed and play
          as usual.
        </p>
        <div className="match-actions">
          <button type="button" className="text-action" onClick={dismiss}>
            Just show my tracks
          </button>
        </div>
      </MatchPanel>
    );
  if (!choosing)
    return (
      <MatchPanel title={`Which release is ${album.title}?`}>
        <p className="match-alert">
          Hito could not match this album to a release by its title. Choose the
          release yourself, or keep your tracks as they are.
        </p>
        <div className="match-actions">
          <button
            type="button"
            className="pill-action"
            data-primary=""
            onClick={opens}
          >
            Choose the release
          </button>
          <button type="button" className="text-action" onClick={dismiss}>
            Just show my tracks
          </button>
        </div>
      </MatchPanel>
    );
  return (
    <ReleasePicker
      title={album.title}
      groups={suggestion.groups}
      pending={suggestion.pending}
      canLoadMore={suggestion.canLoadMore}
      more={() => void suggestion.more()}
      pick={setPicked}
      dismiss={dismiss}
    />
  );
}
