import type { Schema } from "../../../shared/schema";
import { Folder } from "lucide-react";

import ActionMenu from "../../../shared/ui/ActionMenu";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import SettingRow from "../../../shared/ui/SettingRow";

function status(folder: Schema<"MusicFolder">): { label: string } {
  const job = folder.latest_job;
  if (!folder.available)
    return { label: "Unavailable. Check that the drive is connected." };
  if (job?.state === "queued") return { label: "Waiting to scan" };
  if (job?.state === "running")
    return {
      label: `Scanning · ${job.progress.toLocaleString()} files read`,
    };
  if (job?.state === "failed" && job.failure === "media_tools_missing")
    return {
      label:
        "FFmpeg is not installed. Install it (for example `brew install ffmpeg`) and scan again.",
    };
  if (job?.state === "failed")
    return {
      label: "Scan failed. Check the folder and try again.",
    };
  if (job?.state === "complete")
    return {
      label: `${job.progress.toLocaleString()} ${job.progress === 1 ? "file" : "files"} read`,
    };
  return { label: "Ready to scan" };
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
      leading={
        <span className="setting-line-tile" aria-hidden="true">
          <Folder size={18} />
        </span>
      }
      title={
        <h4 title={folder.directory?.components?.join(" / ")}>{folder.name}</h4>
      }
      hint={
        <span role="status">
          {folder.available && folder.latest_job?.state !== "failed"
            ? `${state.label} · read-only`
            : state.label}
        </span>
      }
      action={
        <>
          <Button
            variant="link"
            aria-label={`Scan ${folder.name}`}
            disabled={busy || !folder.available || scanning}
            onClick={onScan}
          >
            Scan
          </Button>
          <ActionMenu
            label={`More for ${folder.name}`}
            triggerClassName="more-button"
            align="end"
            items={[
              {
                label: "Remove folder…",
                destructive: true,
                disabled: busy,
                onSelect: onAskRemove,
              },
            ]}
          />
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
