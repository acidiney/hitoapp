import type { Schema } from "../../../shared/schema";
import { useState } from "react";

import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import { useMissingRecordings } from "../hooks/useMissingRecordings";

function summary(result: Schema<"MissingRecordingsRemoval">): string {
  if (!result.removed && !result.kept_referenced)
    return "No missing tracks to remove.";
  const parts = [
    `Removed ${result.removed.toLocaleString()} ${result.removed === 1 ? "track" : "tracks"}.`,
  ];
  if (result.kept_referenced)
    parts.push(
      `${result.kept_referenced.toLocaleString()} stay because a playlist, like, progress or download still uses them.`,
    );
  if (result.more) parts.push("Remove again to continue.");
  return parts.join(" ");
}

export default function MissingRecordings({
  account,
  library,
  disabled,
}: {
  account: string;
  library: string;
  disabled: boolean;
}) {
  const missing = useMissingRecordings(account, library);
  const [asking, ask] = useState(false);
  return (
    <SettingRow
      title="Tracks with missing files"
      hint="Hito keeps a track for 30 days after its files disappear, then removes it. Playlists, likes, progress and downloads keep a track."
      status={
        <span role="status">
          {missing.result ? summary(missing.result) : ""}
        </span>
      }
      action={
        <Button
          variant="destructive"
          disabled={disabled || missing.removing}
          onClick={() => ask(true)}
        >
          Remove missing tracks now
        </Button>
      }
    >
      {missing.error && (
        <p className="form-error" role="alert">
          {missing.error}
        </p>
      )}
      {asking && (
        <div className="folder-remove-confirm">
          <h4>Remove missing tracks now?</h4>
          <p>
            Tracks whose files cannot be found are removed from your library
            without waiting, together with their artist corrections. Your files
            are not touched. Tracks that a playlist, like, progress or download
            uses stay.
          </p>
          <div>
            <Button disabled={missing.removing} onClick={() => ask(false)}>
              Keep them for now
            </Button>
            <Button
              variant="destructive"
              disabled={missing.removing}
              onClick={async () => {
                await missing.remove();
                ask(false);
              }}
            >
              {missing.removing ? "Removing…" : "Remove missing tracks"}
            </Button>
          </div>
        </div>
      )}
    </SettingRow>
  );
}
