import type { ReactNode } from "react";

export default function AlbumHeader({
  crumbs,
  cover,
  title,
  heading: Heading = "h1",
  credit,
  stats,
  actions,
  notes,
}: {
  crumbs?: ReactNode;
  cover: ReactNode;
  title: string;
  heading?: "h1" | "h4" | "h5";
  credit?: ReactNode;
  stats: ReactNode;
  actions: ReactNode;
  notes?: ReactNode;
}) {
  return (
    <header className="album-header">
      {crumbs}
      <div className="album-header-body">
        <div className="album-header-cover">{cover}</div>
        <div className="album-header-copy">
          <span className="album-eyebrow">Album</span>
          <Heading>{title}</Heading>
          {credit}
          <span className="album-stats">{stats}</span>
          <div className="album-actions">{actions}</div>
          {notes}
        </div>
      </div>
    </header>
  );
}
