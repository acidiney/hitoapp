import type { Schema } from "../../../shared/schema";
import { useLayoutEffect, useRef } from "react";
import { Link, router } from "@inertiajs/react";
import { Play } from "lucide-react";

import type { ProviderSelection, SelectionIntent } from "../hooks/useDiscovery";
import ActionMenu from "../../../shared/ui/ActionMenu";
import { time } from "../../../shared/format";
import Artwork from "../../artwork/components/Artwork";

const knownArtist = (artist: string | null | undefined) =>
  artist && artist.trim().toLowerCase() !== "unknown artist" ? artist : "";

type ResultProps = {
  hit: Schema<"ProviderRecording">;
  saved?: Schema<"Recording">;
  selection: ProviderSelection | null;
  disabled: boolean;
  canPlay: boolean;
  service: string;
  onSelect: (hit: Schema<"ProviderRecording">, intent: SelectionIntent) => void;
};
function ProviderResult({
  hit,
  saved,
  selection,
  disabled,
  canPlay,
  service,
  onSelect,
}: ResultProps) {
  const play = useRef<HTMLButtonElement>(null);
  const save = useRef<HTMLButtonElement>(null);
  const savedLink = useRef<HTMLAnchorElement>(null);
  const restore = useRef<SelectionIntent | null>(null);
  useLayoutEffect(() => {
    if (!restore.current || disabled) return;
    const intent = restore.current;
    restore.current = null;
    if (
      [document.body, play.current, save.current].includes(
        document.activeElement as HTMLElement,
      )
    ) {
      (intent === "play"
        ? play.current
        : saved
          ? savedLink.current
          : save.current
      )?.focus();
    }
  }, [disabled, saved]);
  function select(intent: SelectionIntent) {
    const button = intent === "play" ? play.current : save.current;
    if (document.activeElement === button) restore.current = intent;
    onSelect(hit, intent);
  }
  const current = selection?.id === hit.id ? selection : null;
  const duration =
    hit.duration_ms == null ? null : time(hit.duration_ms / 1000);
  const where = saved ? "in your library" : "not in your library";
  return (
    <li
      className="provider-result"
      tabIndex={-1}
      data-search-row=""
      onKeyDown={(event) => {
        if (
          event.target !== event.currentTarget ||
          event.key !== "Enter" ||
          disabled ||
          !canPlay
        )
          return;
        event.preventDefault();
        select(event.metaKey || event.ctrlKey ? "queue" : "play");
      }}
    >
      <Artwork
        recording={saved?.id}
        fallback={{
          id: hit.release?.id ?? hit.id,
          title: hit.release?.title ?? hit.title,
          artist: hit.artist,
        }}
      />
      <div className="provider-result-copy">
        <h3>{hit.title}</h3>
        <p>
          {[
            knownArtist(hit.artist) || knownArtist(saved?.artist),
            service,
            where,
          ]
            .filter(Boolean)
            .join(" · ")}
        </p>
        {current?.pending && (
          <small role="status">
            {current.intent === "play"
              ? "Preparing playback…"
              : current.intent === "save"
                ? "Adding to your library…"
                : "Adding to your queue…"}
          </small>
        )}
      </div>
      <span
        className="provider-duration"
        aria-label={duration ? `Duration ${duration}` : undefined}
        aria-hidden={duration ? undefined : true}
      >
        {duration}
      </span>
      <button
        className="icon-button provider-play"
        data-unplayable={canPlay ? undefined : ""}
        ref={play}
        aria-label={`Play ${hit.title}`}
        disabled={disabled || !canPlay}
        onClick={() => select("play")}
      >
        <Play size={14} fill="currentColor" />
      </button>
      <div className="provider-result-action">
        {saved ? (
          <Link
            className="text-link"
            ref={savedLink}
            href={`/recordings/${encodeURIComponent(saved.id)}`}
            aria-label={`View ${hit.title} in library`}
          >
            Open
          </Link>
        ) : (
          <button
            type="button"
            className="text-link"
            ref={save}
            aria-label={`Add ${hit.title} to library`}
            disabled={disabled}
            onClick={() => select("save")}
          >
            Add to library
          </button>
        )}
      </div>
      <ActionMenu
        label={`Actions for ${hit.title}`}
        triggerClassName="icon-button provider-menu"
        align="end"
        items={[
          saved
            ? {
                label: "Open in library",
                onSelect: () =>
                  router.visit(`/recordings/${encodeURIComponent(saved.id)}`),
              }
            : {
                label: "Add to library",
                disabled,
                onSelect: () => select("save"),
              },
          {
            label: "Play next",
            disabled: disabled || !canPlay,
            onSelect: () => select("play_next"),
          },
          {
            label: "Add to queue",
            disabled: disabled || !canPlay,
            onSelect: () => select("queue"),
          },
        ]}
      />
      {current?.error && (
        <p className="provider-result-error" role="alert">
          {current.error}{" "}
          <Link
            className="text-link"
            href="/discover"
            only={["connections", "service_names"]}
            preserveState
            preserveScroll
            replace
          >
            Refresh services
          </Link>
        </p>
      )}
    </li>
  );
}
export default function ProviderResults({
  items,
  saved,
  selection,
  pending,
  canPlay,
  service,
  onSelect,
}: {
  items: Schema<"ProviderRecording">[];
  saved: ReadonlyMap<string, Schema<"Recording">>;
  selection: ProviderSelection | null;
  pending: boolean;
  canPlay: boolean;
  service: string;
  onSelect: ResultProps["onSelect"];
}) {
  const disabled = pending || Boolean(selection?.pending);
  return (
    <ol
      className="provider-results"
      aria-label="Music search results"
      aria-busy={pending}
    >
      {items.map((hit) => (
        <ProviderResult
          key={hit.id}
          hit={hit}
          saved={saved.get(hit.id)}
          selection={selection}
          disabled={disabled}
          canPlay={canPlay}
          service={service}
          onSelect={onSelect}
        />
      ))}
    </ol>
  );
}
