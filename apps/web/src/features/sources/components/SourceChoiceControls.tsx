import type { Schema } from "../../../shared/schema";
import { useId } from "react";
import { Link } from "@inertiajs/react";

import ActionMenu from "../../../shared/ui/ActionMenu";
import Button from "../../../shared/ui/Button";
import { useSameRecording } from "../hooks/useSameRecording";
import SameRecordingDialog from "./SameRecordingDialog";
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
  account: string;
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
  account,
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
  const same = useSameRecording(
    account,
    (choice?.sources ?? []).map((entry) => entry.source),
  );
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
                    {same.offered && entry.source.community_public && (
                      <ActionMenu
                        label={`More for ${labels.primary}`}
                        align="end"
                        items={[
                          {
                            label: "Suggest as same recording",
                            disabled:
                              same.consent !== true ||
                              same.rows[entry.source.id]?.kind === "sending",
                            onSelect: () => same.pick(entry.source),
                          },
                        ]}
                      />
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
                  <SameRecordingStatus
                    row={same.rows[entry.source.id]}
                    onRetry={(other) => void same.send(entry.source.id, other)}
                    onCancel={same.cancel}
                  />
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
          {same.offered && same.consent === false && (
            <p className="track-note">
              To suggest that two sources are the same recording, turn on
              Metadata corrections in{" "}
              <Link href="/settings?section=privacy">Community settings</Link>.
            </p>
          )}
          {same.picking && (
            <SameRecordingDialog
              from={same.picking}
              candidates={same.candidates}
              onSend={(other) => void same.send(same.picking!.id, other)}
              onClose={() => same.pick(null)}
            />
          )}
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

function SameRecordingStatus({
  row,
  onRetry,
  onCancel,
}: {
  row: ReturnType<typeof useSameRecording>["rows"][string] | undefined;
  onRetry(other: string): void;
  onCancel(): void;
}) {
  if (!row) return null;
  if (row.kind === "sending")
    return (
      <span className="rd-choice-note" role="status">
        Sending suggestion…{" "}
        <Button variant="link" onClick={onCancel}>
          Cancel
        </Button>
      </span>
    );
  if (row.kind === "sent")
    return (
      <span className="rd-choice-note" role="status">
        Suggestion sent for review
      </span>
    );
  return (
    <span className="rd-choice-note">
      <span className="form-error" role="alert">
        {row.error}
      </span>{" "}
      <Button variant="link" onClick={() => onRetry(row.other)}>
        Try again
      </Button>
    </span>
  );
}
