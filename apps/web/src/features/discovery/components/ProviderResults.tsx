import { useLayoutEffect, useRef } from "react";
import { Link } from "@inertiajs/react";
import { Check, Play, Plus } from "lucide-react";
import type { ProviderRecording, Recording } from "../../../shared/types";
import type { ProviderSelection } from "../hooks/useDiscovery";
import { time } from "../../../shared/format";
import Artwork from "../../artwork/components/Artwork";
import AvailabilityLabel from "../../../shared/ui/AvailabilityLabel";
import Button from "../../../shared/ui/Button";

type SelectionIntent = "play" | "save";
type ResultProps = {
  hit: ProviderRecording;
  saved?: Recording;
  selection: ProviderSelection | null;
  disabled: boolean;
  canPlay: boolean;
  onSelect: (hit: ProviderRecording, intent: SelectionIntent) => void;
};
function ProviderResult({
  hit,
  saved,
  selection,
  disabled,
  canPlay,
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
  return (
    <li className="provider-result">
      <button
        className="icon-button provider-play"
        ref={play}
        aria-label={`Play ${hit.title}`}
        disabled={disabled || !canPlay}
        onClick={() => select("play")}
      >
        <Play size={18} fill="currentColor" />
      </button>
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
        <p>{hit.artist}</p>
        {hit.release && <small>{hit.release.title}</small>}
        {current?.pending && (
          <small role="status">
            {current.intent === "play"
              ? "Preparing playback…"
              : "Adding to your library…"}
          </small>
        )}
      </div>
      <span
        className="provider-duration"
        aria-label={duration ? `Duration ${duration}` : "Duration unknown"}
      >
        {duration ?? "—"}
      </span>
      <div className="provider-result-state">
        <AvailabilityLabel
          state={saved ? "library" : canPlay ? "available" : "catalog"}
        />
      </div>
      <div className="provider-result-action">
        {saved ? (
          <Link
            className="text-link"
            ref={savedLink}
            href={`/recordings/${encodeURIComponent(saved.id)}`}
            aria-label={`View ${hit.title} in library`}
          >
            <Check size={16} />
            Open
          </Link>
        ) : (
          <Button
            ref={save}
            aria-label={`Add ${hit.title} to library`}
            disabled={disabled}
            onClick={() => select("save")}
          >
            <Plus size={16} />
            Add
          </Button>
        )}
      </div>
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
  onSelect,
}: {
  items: ProviderRecording[];
  saved: ReadonlyMap<string, Recording>;
  selection: ProviderSelection | null;
  pending: boolean;
  canPlay: boolean;
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
          onSelect={onSelect}
        />
      ))}
    </ol>
  );
}
