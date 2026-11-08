import type { Schema } from "../../../shared/schema";

import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import HealthDot, { type HealthTone } from "../../../shared/ui/HealthDot";
import SettingRow from "../../../shared/ui/SettingRow";

function status(folder: Schema<"MusicFolder">): {
  tone: HealthTone;
  label: string;
} {
  const job = folder.latest_job;
  if (!folder.available)
    return {
      tone: "warning",
      label: "Unavailable. Check that the drive is connected.",
    };
  if (job?.state === "queued")
    return { tone: "action", label: "Waiting to scan" };
  if (job?.state === "running")
    return {
      tone: "action",
      label: `Scanning · ${job.progress.toLocaleString()} files read`,
    };
  if (job?.state === "failed" && job.failure === "media_tools_missing")
    return {
      tone: "warning",
      label:
        "FFmpeg is not installed. Install it (for example `brew install ffmpeg`) and scan again.",
    };
  if (job?.state === "failed")
    return {
      tone: "warning",
      label: "Scan failed. Check the folder and try again.",
    };
  if (job?.state === "complete")
    return {
      tone: "success",
      label: `${job.progress.toLocaleString()} ${job.progress === 1 ? "file" : "files"} read`,
    };
  return { tone: "action", label: "Ready to scan" };
}

export default function FolderRow({
  folder,
  busy,
  pending,
  removing,
  error = "",
  onScan,
  onAskRemove,
  onCancelRemove,
  onRemove,
}: {
  folder: Schema<"MusicFolder">;
  busy: boolean;
  pending: boolean;
  removing: boolean;
  error?: string;
  onScan: () => void;
  onAskRemove: () => void;
  onCancelRemove: () => void;
  onRemove: () => void;
}) {
  const state = status(folder);
  const scanning =
    folder.latest_job?.state === "queued" ||
    folder.latest_job?.state === "running";
  return (
    <SettingRow
      title={<h4>{folder.name}</h4>}
      hint={
        !!folder.directory?.components?.length && (
          <span className="setting-line-path">
            {folder.directory.components.join(" / ")}
          </span>
        )
      }
      status={
        <span role="status">
          <HealthDot tone={state.tone} label={state.label} />
        </span>
      }
      action={
        <>
          <Button
            aria-label={`Scan ${folder.name}`}
            disabled={busy || !folder.available || scanning}
            onClick={onScan}
          >
            Scan folder
          </Button>
          <Button
            variant="destructive"
            aria-label={`Remove ${folder.name}`}
            disabled={busy}
            onClick={onAskRemove}
          >
            Remove
          </Button>
        </>
      }
    >
      {removing && (
        <Dialog
          title={`Remove ${folder.name}?`}
          busy={pending}
          onClose={onCancelRemove}
        >
          <p className="muted">
            Your files stay on disk. Playlists, likes, and library records are
            kept. Music available only in this folder will become unavailable.
          </p>
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          <div className="dialog-actions">
            <Button disabled={pending} onClick={onCancelRemove}>
              Keep folder
            </Button>
            <Button variant="destructive" disabled={busy} onClick={onRemove}>
              {pending ? "Removing…" : "Remove folder"}
            </Button>
          </div>
        </Dialog>
      )}
    </SettingRow>
  );
}
