import type { ReactNode } from "react";

export default function PlaylistHeader({
  cover,
  eyebrow,
  title,
  nameField,
  total,
  ready,
  actions,
}: {
  cover: ReactNode;
  eyebrow: string;
  title: string;
  nameField?: ReactNode;
  total: number;
  ready: number;
  actions: ReactNode;
}) {
  const waiting = total - ready;
  return (
    <header className="playlist-header">
      <div className="playlist-header-cover">{cover}</div>
      <div className="playlist-header-copy">
        <span className="playlist-header-eyebrow">{eyebrow}</span>
        {nameField ? (
          <>
            <h1 className="sr-only">{title}</h1>
            {nameField}
          </>
        ) : (
          <h1>{title}</h1>
        )}
        <div className="playlist-header-progress">
          {waiting > 0 && (
            <div
              className="ready-bar"
              role="img"
              aria-label={`${ready} of ${total} tracks ready to play`}
            >
              <div style={{ width: `${(ready / total) * 100}%` }} />
            </div>
          )}
          <span className="playlist-header-stats">
            {total} {total === 1 ? "track" : "tracks"} ·{" "}
            {waiting > 0 ? (
              <>
                <strong>{ready} ready</strong> · {waiting} not ready yet
              </>
            ) : (
              <strong>all ready</strong>
            )}
          </span>
        </div>
        <div className="playlist-header-actions">{actions}</div>
      </div>
    </header>
  );
}
