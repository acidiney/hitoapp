import { Folder, FolderSync, Trash2 } from "lucide-react";
import type { MusicFolder } from "../../../shared/types";
import Button from "../../../shared/ui/Button";
import HealthDot, { type HealthTone } from "../../../shared/ui/HealthDot";
import SettingRow from "../../../shared/ui/SettingRow";

function status(folder: MusicFolder): { tone: HealthTone; label: string } {
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
  onScan,
  onAskRemove,
  onCancelRemove,
  onRemove,
}: {
  folder: MusicFolder;
  busy: boolean;
  pending: boolean;
  removing: boolean;
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
      title={
        <span className="setting-line-icon">
          <Folder size={18} aria-hidden="true" />
          <h4>{folder.name}</h4>
        </span>
      }
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
          <button
            className="icon-button"
            aria-label={`Scan ${folder.name}`}
            disabled={busy || !folder.available || scanning}
            onClick={onScan}
          >
            <FolderSync size={19} />
          </button>
          <button
            className="icon-button"
            aria-label={`Remove ${folder.name}`}
            disabled={busy}
            onClick={onAskRemove}
          >
            <Trash2 size={18} />
          </button>
        </>
      }
    >
      {removing && (
        <div className="folder-remove-confirm">
          <h4>Remove {folder.name}?</h4>
          <p>
            Your files stay on disk. Playlists, likes, and library records are
            kept. Music available only in this folder will become unavailable.
          </p>
          <div>
            <Button disabled={busy} onClick={onCancelRemove}>
              Keep folder
            </Button>
            <Button variant="destructive" disabled={busy} onClick={onRemove}>
              {pending ? "Removing…" : "Remove folder"}
            </Button>
          </div>
        </div>
      )}
    </SettingRow>
  );
}
