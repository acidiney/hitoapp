import type { Schema } from "../../../shared/schema";
import { useState } from "react";

import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
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
      `${result.kept_referenced.toLocaleString()} stay because a playlist, like, progress, download or edit of yours still uses them.`,
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
        <Dialog
          title="Remove missing tracks now?"
          busy={missing.removing}
          onClose={() => ask(false)}
        >
          <p className="muted">
            Missing tracks that nothing uses are removed from your library
            without waiting. Your files are not touched. Tracks in a playlist,
            liked, with progress, downloads, or your own title or artist edits
            stay. Every music folder must be connected and finished scanning, or
            Hito will not remove anything.
          </p>
          <div className="dialog-actions">
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
        </Dialog>
      )}
    </SettingRow>
  );
}
