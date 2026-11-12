import type { ReactNode } from "react";

export default function AlbumHeader({
  eyebrow = "Album",
  cover,
  title,
  heading: Heading = "h1",
  credit,
  stats,
  status,
  actions,
  notes,
}: {
  eyebrow?: string;
  cover: ReactNode;
  title: string;
  heading?: "h1" | "h4" | "h5";
  credit?: ReactNode;
  stats: ReactNode;
  status?: ReactNode;
  actions: ReactNode;
  notes?: ReactNode;
}) {
  return (
    <header className="album-header">
      <div className="album-header-cover">{cover}</div>
      <div className="album-header-copy">
        <span className="album-eyebrow">{eyebrow}</span>
        <Heading data-long={title.length > 28 || undefined}>{title}</Heading>
        <span className="album-stats">
          {credit}
          {credit && stats ? " · " : null}
          {stats}
        </span>
        {status}
        <div className="album-actions">{actions}</div>
        {notes}
      </div>
    </header>
  );
}
