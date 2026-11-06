import type { Schema } from "../../../shared/schema";
import { useId } from "react";

import Button from "../../../shared/ui/Button";
import AvailabilityLabel from "../../../shared/ui/AvailabilityLabel";

const modes: Record<Schema<"SourceChoice">["mode"], string> = {
  automatic: "Automatic source selection",
  manual: "Manual source selection",
  reviewed: "Reviewed sources",
};

type Props = {
  choice: Schema<"SourceChoice"> | null;
  stagedSource: string | null;
  checking: boolean;
  pending: boolean;
  retrying: boolean;
  unavailable: boolean;
  error: string;
  statusError: string;
  blocked: boolean;
  canChoose: boolean;
  canReset: boolean;
  stage(source: string): void;
  choose(): Promise<void>;
  reset(): Promise<void>;
  retry(): Promise<void>;
  check(): Promise<void>;
};

function savedSelection(choice: Schema<"SourceChoice">, source: string) {
  return choice.sources.some(
    (entry) => entry.selected && entry.source.id === source,
  );
}

export default function SourceChoiceControls({
  choice,
  stagedSource,
  checking,
  pending,
  retrying,
  unavailable,
  error,
  statusError,
  blocked,
  canChoose,
  canReset,
  stage,
  choose,
  reset,
  retry,
  check,
}: Props) {
  const heading = useId();
  const selectedSource =
    stagedSource ??
    (choice?.mode === "manual"
      ? (
          choice.sources.find((entry) => entry.selected && entry.selectable) ??
          choice.sources.find((entry) => entry.selected)
        )?.source.id
      : null);
  if (unavailable) return null;
  return (
    <section className="rd-card" aria-labelledby={heading}>
      <h2 id={heading}>Playback source</h2>
      {choice && <p>{modes[choice.mode]}</p>}
      <p role="status">
        {pending
          ? "Saving source choice…"
          : checking
            ? "Checking source choice…"
            : null}
      </p>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {statusError && (
        <p className="form-error" role="alert">
          {statusError}
        </p>
      )}
      {choice && (
        <>
          <ul className="rd-rows rd-choices">
            {choice.sources.map((entry) => (
              <li className="rd-row rd-choice" key={entry.source.id}>
                <label>
                  <input
                    type="radio"
                    name={`playback-source-${heading}`}
                    checked={selectedSource === entry.source.id}
                    disabled={blocked || !entry.selectable}
                    onChange={() => stage(entry.source.id)}
                  />{" "}
                  {entry.source.label}
                </label>
                <span className="rd-choice-origin">
                  {entry.source.provider}
                </span>
                <span className="rd-choice-state">
                  <AvailabilityLabel
                    state={
                      entry.source.device_id
                        ? "library"
                        : entry.source.ready
                          ? "available"
                          : "catalog"
                    }
                    label={
                      entry.source.device_id
                        ? "On this device"
                        : entry.source.ready
                          ? "Ready"
                          : "Not prepared"
                    }
                  />
                  {choice.mode === "manual" && entry.selected && (
                    <span className="track-chip" data-tone="neutral">
                      Pinned
                    </span>
                  )}
                </span>
              </li>
            ))}
          </ul>
          {stagedSource !== null && !savedSelection(choice, stagedSource) && (
            <p role="status">
              Selected, not saved yet. Use source to play from it.
            </p>
          )}
          <div className="rd-actions">
            {choice.mode !== "automatic" && (
              <Button disabled={!canReset} onClick={() => void reset()}>
                Return to automatic
              </Button>
            )}
            <Button
              variant="primary"
              disabled={!canChoose}
              onClick={() => void choose()}
            >
              Use source
            </Button>
          </div>
        </>
      )}
      {retrying && (
        <Button disabled={pending || checking} onClick={() => void retry()}>
          Retry source choice
        </Button>
      )}
      {statusError && !retrying && (
        <Button disabled={pending || checking} onClick={() => void check()}>
          Check source choice
        </Button>
      )}
    </section>
  );
}
