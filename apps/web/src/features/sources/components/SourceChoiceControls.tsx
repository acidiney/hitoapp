import type { Schema } from "../../../shared/schema";
import { useId } from "react";

import Button from "../../../shared/ui/Button";
import {
  bestGuessReason,
  sourceLabels,
  sourceState,
} from "../model/sourceLabels";

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
  fixSource(): void;
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
  fixSource,
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
      <div className="track-block-head">
        <h2 id={heading}>Playback source</h2>
        {choice && <p className="track-muted">{modes[choice.mode]}</p>}
      </div>
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
            {choice.sources.map((entry) => {
              const labels = sourceLabels(entry.source);
              return (
                <li className="rd-row rd-choice" key={entry.source.id}>
                  <label>
                    <input
                      type="radio"
                      name={`playback-source-${heading}`}
                      checked={selectedSource === entry.source.id}
                      disabled={blocked || !entry.selectable}
                      onChange={() => stage(entry.source.id)}
                    />{" "}
                    {labels.primary}
                  </label>
                  <span className="rd-choice-origin">{labels.secondary}</span>
                  <span className="rd-choice-state">
                    <span
                      className="track-chip"
                      data-tone={sourceState(entry.source).tone}
                    >
                      {sourceState(entry.source).label}
                    </span>
                    {choice.mode === "manual" && entry.selected && (
                      <span className="track-chip" data-tone="action">
                        Pinned
                      </span>
                    )}
                    {entry.source.match_kind === "best_guess" && (
                      <>
                        <span
                          className="track-chip"
                          data-tone="warning"
                          title={bestGuessReason(entry.source)}
                        >
                          Best guess
                        </span>
                        <Button variant="secondary" onClick={fixSource}>
                          Fix source
                        </Button>
                      </>
                    )}
                  </span>
                </li>
              );
            })}
          </ul>
          <div
            className="rd-actions"
            hidden={
              choice.mode === "automatic" &&
              stagedSource === null &&
              choice.sources.length < 2
            }
          >
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
          {stagedSource !== null && !savedSelection(choice, stagedSource) && (
            <p className="track-note" role="status">
              Selected, not saved yet. Use source to play from it.
            </p>
          )}
          {choice.mode === "automatic" &&
            stagedSource === null &&
            choice.sources.length === 1 && (
              <p className="track-note">
                Automatic picks the best ready source. With one source there is
                nothing to choose.
              </p>
            )}
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
