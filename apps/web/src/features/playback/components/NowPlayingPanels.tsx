import { useSyncExternalStore } from "react";
import { usePlayer } from "./PlaybackProvider";
import { useQueuePage } from "../hooks/useQueuePage";
import Artwork from "../../artwork/components/Artwork";
import { coverIdentity } from "../../artwork/coverIdentity";
import SegmentedControl from "../../../shared/ui/SegmentedControl";
import RouteLink from "../../../shared/ui/RouteLink";
import { time } from "../../../shared/format";
import { SPEED_CHOICES } from "./SpeedControl";
import type { Track } from "../model";

export function UpNextPanel({ openQueue }: { openQueue: () => void }) {
  const player = usePlayer();
  const { page, error, loading, retry, view } = useQueuePage(player);
  const entries = page?.entries ?? [];
  return (
    <div className="np-next">
      {loading && <p className="queue-empty">Loading your queue…</p>}
      {error && (
        <div className="queue-empty" role="alert">
          <p>{error}</p>
          <button className="text-link" onClick={retry}>
            Retry queue
          </button>
        </div>
      )}
      {!loading && !error && entries.length === 0 && (
        <p className="queue-empty">
          {view.total
            ? "Nothing is queued after this track."
            : "Choose a track to start listening."}
        </p>
      )}
      {entries.length > 0 && (
        <ol>
          {entries.map(({ id, recording }) => (
            <li key={id}>
              <button
                type="button"
                className="np-next-row"
                onClick={() => void player.selectEntry(id)}
              >
                <Artwork
                  release={recording.release_id ?? undefined}
                  recording={recording.id}
                  fallback={coverIdentity(recording)}
                />
                <span>
                  <strong>{recording.title}</strong>
                  <small>{recording.artist}</small>
                </span>
                <span className="mono">
                  {time(recording.duration_ms / 1000)}
                </span>
              </button>
            </li>
          ))}
        </ol>
      )}
      <RouteLink href="/queue" className="np-queue-link" onNavigate={openQueue}>
        See full queue
      </RouteLink>
    </div>
  );
}

export function LyricsPanel({ track }: { track: Track | null }) {
  return (
    <div className="np-lyrics">
      <p>Lyrics are not shown here.</p>
      <p className="muted">
        Nothing is sent to a lyrics provider until you ask. Open the track
        details to choose a provider and load lyrics.
      </p>
      {track && (
        <RouteLink href={`/recordings/${encodeURIComponent(track.id)}`}>
          Track details
        </RouteLink>
      )}
    </div>
  );
}

const label = (speed: number) => `${speed}×`;

export function SourcePanel({ track }: { track: Track | null }) {
  const player = usePlayer();
  const speed = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().speed,
  );
  const effective = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().effectiveSpeed ?? player.snapshot().speed,
  );
  const range = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().speedRange ?? null,
  );
  const ready = useSyncExternalStore(player.subscribe, () => {
    const state = player.snapshot();
    return state.connection === "ready" && state.controlsAvailable !== false;
  });
  const choices = SPEED_CHOICES.filter(
    (choice) =>
      choice === speed ||
      range === null ||
      (choice >= range.min && choice <= range.max),
  );
  return (
    <div className="np-source">
      <section className="np-card" aria-label="This source">
        <h2>This source</h2>
        <dl>
          <dt>player</dt>
          <dd>
            {player.mode === "native" ? "Desktop player" : "This browser"}
          </dd>
          <dt>plays at</dt>
          <dd>{label(effective)}</dd>
          {range && (
            <>
              <dt>speeds</dt>
              <dd>{`${range.min}–${range.max}×`}</dd>
            </>
          )}
        </dl>
        {track && (
          <p className="np-card-foot">
            <RouteLink href={`/recordings/${encodeURIComponent(track.id)}`}>
              Choose another source in track details
            </RouteLink>
          </p>
        )}
      </section>
      <section className="np-card" aria-label="Speed">
        <h2>Speed</h2>
        <SegmentedControl
          label="Speed"
          options={choices.map((choice) => ({
            value: String(choice),
            label: label(choice),
          }))}
          value={String(speed)}
          disabled={!player.supports.speed || !ready}
          onChange={(value) => player.speed(+value)}
        />
      </section>
    </div>
  );
}
