import type { AlbumProgress } from "./AlbumMergeContext";

export default function AlbumProgressLine({
  progress,
}: {
  progress: AlbumProgress;
}) {
  const { owned, total, elsewhere } = progress;
  const percent = total === 0 ? 0 : Math.round((owned / total) * 100);
  return (
    <span className="album-status" data-tone="progress">
      <span
        className="album-progress"
        role="img"
        aria-label={`${owned} of ${total} tracks in your library`}
      >
        <span style={{ width: `${percent}%` }} />
      </span>
      <span>
        {owned} of {total} in your library
        {elsewhere > 0 ? ` · ${elsewhere} more not in this edition` : ""}
      </span>
    </span>
  );
}
