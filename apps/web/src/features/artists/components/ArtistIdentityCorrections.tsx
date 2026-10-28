import { useState } from "react";
import { router } from "@inertiajs/react";
import { useArtistIdentityCorrections } from "../hooks/useArtistIdentityCorrections";
import type { ArtistIdentityGroupDetail } from "../../../shared/types";

export default function ArtistIdentityCorrections({
  account,
  admin,
  detail,
}: {
  account: string;
  admin: boolean;
  detail: ArtistIdentityGroupDetail;
}) {
  const [opened, setOpened] = useState(false);
  const [recordings, setRecordings] = useState<string[]>([]);
  const [releases, setReleases] = useState<string[]>([]);
  const [destination, setDestination] = useState("credit");
  const [search, setSearch] = useState("");
  const corrections = useArtistIdentityCorrections(
    account,
    detail.group.id,
    admin,
    opened,
    () => {
      setRecordings([]);
      setReleases([]);
      router.reload();
    },
  );
  const { changes, groups, shared, pending, retrying, error, notice } =
    corrections;
  const latest = changes?.latest;
  const busy = pending !== null;
  const target = groups.find((item) => item.id === destination);
  const ready =
    changes !== null &&
    (recordings.length > 0 || releases.length > 0) &&
    (destination === "credit" || target !== undefined);

  function toggle(list: string[], set: (value: string[]) => void, id: string) {
    set(list.includes(id) ? list.filter((item) => item !== id) : [...list, id]);
  }

  if (!opened)
    return (
      <section aria-labelledby="group-corrections-title">
        <h2 className="discography-heading" id="group-corrections-title">
          Corrections
        </h2>
        <button
          type="button"
          className="button"
          onClick={() => setOpened(true)}
        >
          Correct this artist
        </button>
      </section>
    );

  return (
    <section aria-labelledby="group-corrections-title">
      <h2 className="discography-heading" id="group-corrections-title">
        Corrections
      </h2>
      <p>
        Move tracks that belong to a different artist. Recordings, playlists,
        likes, progress and your edits do not change, and every move can be
        undone.
      </p>
      <fieldset disabled={busy}>
        <legend>Tracks on this page</legend>
        {detail.recordings.items.map(({ recording }) => (
          <label key={recording.id}>
            <input
              type="checkbox"
              checked={recordings.includes(recording.id)}
              onChange={() => toggle(recordings, setRecordings, recording.id)}
            />{" "}
            Move {recording.title}
          </label>
        ))}
      </fieldset>
      {detail.releases.items.length > 0 && (
        <fieldset disabled={busy}>
          <legend>Albums on this page</legend>
          {detail.releases.items.map((release) => (
            <label key={release.id}>
              <input
                type="checkbox"
                checked={releases.includes(release.id)}
                onChange={() => toggle(releases, setReleases, release.id)}
              />{" "}
              Move tracks on {release.title}
            </label>
          ))}
        </fieldset>
      )}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          corrections.findGroups(search.trim());
        }}
      >
        <label>
          Find destination artist{" "}
          <input
            type="search"
            value={search}
            maxLength={200}
            disabled={busy}
            onChange={(event) => setSearch(event.target.value)}
          />
        </label>{" "}
        <button type="submit" className="button" disabled={busy}>
          Search artists
        </button>
      </form>
      <label>
        Move to{" "}
        <select
          value={destination}
          disabled={busy}
          onChange={(event) => setDestination(event.target.value)}
        >
          <option value="credit">Each track&apos;s library credit</option>
          {groups.map((item) => (
            <option key={item.id} value={item.id}>
              {item.name.value} ({item.recording_count}{" "}
              {item.recording_count === 1 ? "track" : "tracks"})
            </option>
          ))}
        </select>
      </label>
      {corrections.groupTotal > 20 && (
        <nav aria-label="Destination artist pages">
          <button
            type="button"
            className="button"
            disabled={corrections.groupPage.offset === 0}
            onClick={() => corrections.moveGroupPage(-1)}
          >
            Earlier artists
          </button>
          <button
            type="button"
            className="button"
            disabled={corrections.groupPage.offset + 20 >= corrections.groupTotal}
            onClick={() => corrections.moveGroupPage(1)}
          >
            More artists
          </button>
        </nav>
      )}
      <button
        type="button"
        className="button"
        disabled={busy || retrying || !ready}
        onClick={() =>
          corrections.split({
            recordings,
            releases,
            destination: target
              ? {
                  kind: "identity",
                  provider: target.provider,
                  provider_artist_id: target.provider_artist_id,
                }
              : { kind: "credit" },
          })
        }
      >
        Move selected tracks
      </button>
      {latest && (
        <p>
          Latest change:{" "}
          {latest.kind === "split" ? "moved" : "restored"} {latest.recordings}{" "}
          {latest.recordings === 1 ? "track" : "tracks"}
          {latest.retired > 0 &&
            `. ${latest.retired} no longer ${latest.retired === 1 ? "applies" : "apply"} because the file tags changed.`}
        </p>
      )}
      {latest?.undoable && (
        <button
          type="button"
          className="button"
          disabled={busy || retrying}
          onClick={() => corrections.undo()}
        >
          Undo last artist change
        </button>
      )}
      {admin && latest?.shareable && (
        <button
          type="button"
          className="button"
          disabled={busy || retrying}
          onClick={() => corrections.share()}
        >
          Apply this correction for everyone
        </button>
      )}
      {admin && latest?.shared && (
        <>
          <p>This correction applies to every account.</p>
          <button
            type="button"
            className="button"
            disabled={busy || retrying}
            onClick={() => corrections.withdraw()}
          >
            Withdraw the shared correction
          </button>
        </>
      )}
      {admin && shared && shared.items.some((item) => item.retired > 0) && (
        <p>Some shared corrections no longer match their files&apos; tags.</p>
      )}
      {error && <p role="alert">{error}</p>}
      {retrying && (
        <button
          type="button"
          className="button"
          disabled={busy}
          onClick={() => corrections.retry()}
        >
          Retry the same artist correction request
        </button>
      )}
      {notice && <p role="status">{notice}</p>}
    </section>
  );
}
