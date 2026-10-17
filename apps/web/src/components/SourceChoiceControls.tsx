import { useId } from "react";
import type { SourceChoice } from "../types";

const modes: Record<SourceChoice["mode"], string> = {
  automatic: "Automatic source selection",
  manual: "Manual source selection",
  reviewed: "Reviewed sources",
};

type Props = {
  choice: SourceChoice | null;
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
    <section aria-labelledby={heading}>
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
          <ul>
            {choice.sources.map((entry) => (
              <li key={entry.source.id}>
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
              </li>
            ))}
          </ul>
          <button
            className="button"
            disabled={!canChoose}
            onClick={() => void choose()}
          >
            Use source
          </button>
          {choice.mode !== "automatic" && (
            <button
              className="button"
              disabled={!canReset}
              onClick={() => void reset()}
            >
              Return to automatic
            </button>
          )}
        </>
      )}
      {retrying && (
        <button
          className="button"
          disabled={pending || checking}
          onClick={() => void retry()}
        >
          Retry source choice
        </button>
      )}
      {statusError && !retrying && (
        <button
          className="button"
          disabled={pending || checking}
          onClick={() => void check()}
        >
          Check source choice
        </button>
      )}
    </section>
  );
}
