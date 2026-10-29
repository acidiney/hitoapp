import type { CSSProperties } from "react";
import { coverStyle } from "./coverPalette";

export default function FallbackCover({
  id,
  title,
  artist,
}: {
  id: string;
  title: string;
  artist?: string | null;
}) {
  const style = coverStyle(id);
  const colours = {
    "--cover-bg": style.background,
    "--cover-fg": style.foreground,
    "--cover-accent": style.accent,
  } as CSSProperties;
  return (
    <div
      className="fallback-cover"
      role="img"
      aria-label={`Cover of ${title}`}
      data-shape={style.shape}
      style={colours}
    >
      {artist && <span className="fallback-cover-artist">{artist}</span>}
      <span className="fallback-cover-title">{title}</span>
    </div>
  );
}
