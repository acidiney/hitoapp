import type { ReactNode } from "react";

export default function GenreHeader({
  cover,
  crumbs,
  title,
  stats,
  actions,
}: {
  cover: ReactNode;
  crumbs: ReactNode;
  title: string;
  stats: string;
  actions: ReactNode;
}) {
  return (
    <header className="playlist-header genre-header">
      <div className="playlist-header-cover">{cover}</div>
      <div className="playlist-header-copy">
        <span className="playlist-header-eyebrow">{crumbs}</span>
        <h1>{title}</h1>
        <span className="playlist-header-stats">{stats}</span>
        <div className="playlist-header-actions">{actions}</div>
      </div>
    </header>
  );
}
