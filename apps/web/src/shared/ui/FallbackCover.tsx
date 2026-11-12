import { memo, useMemo, type CSSProperties } from "react";
import { Heart, ListMusic, Music, User } from "lucide-react";
import CoverArtSvg from "./CoverArtSvg";
import { coverArt, type CoverArtKind } from "./coverArt";

export type FallbackCoverKind = CoverArtKind | "liked";

function Glyph({ kind }: { kind: FallbackCoverKind }) {
  if (kind === "playlist") return <ListMusic strokeWidth={1.6} aria-hidden />;
  if (kind === "artist") return <User strokeWidth={1.6} aria-hidden />;
  if (kind === "liked")
    return <Heart strokeWidth={1.6} fill="currentColor" aria-hidden />;
  return <Music strokeWidth={1.6} aria-hidden />;
}

function label(kind: FallbackCoverKind, title: string): string {
  if (kind === "artist") return `Photo of ${title}`;
  if (kind === "liked") return "Liked tracks cover";
  return `Cover of ${title}`;
}

function GeneratedCover({
  id,
  title,
  artist,
  inline,
  monogramOnly,
}: {
  id: string;
  title: string;
  artist?: string | null;
  inline: boolean;
  monogramOnly: boolean;
}) {
  const art = useMemo(
    () => coverArt("genre", id, title, artist),
    [id, title, artist],
  );
  const style = { "--cover-base": art.paint.base } as CSSProperties;
  return (
    <div
      className="fallback-cover"
      role="img"
      aria-label={`Cover of ${title}`}
      data-kind="genre"
      style={style}
    >
      <CoverArtSvg art={art} />
      {inline && !monogramOnly && (
        <div className="fallback-cover-text" data-scale={art.text.scale}>
          {art.text.subtitle && (
            <span className="fallback-cover-subtitle">{art.text.subtitle}</span>
          )}
          <span className="fallback-cover-title">{art.text.title}</span>
        </div>
      )}
      {inline && (
        <span
          className="fallback-cover-monogram"
          data-always={monogramOnly || undefined}
        >
          {art.monogram}
        </span>
      )}
    </div>
  );
}

function FallbackCoverView({
  id,
  title,
  artist,
  kind = "album",
  inline = true,
  monogramOnly = false,
}: {
  id: string;
  title: string;
  artist?: string | null;
  kind?: FallbackCoverKind;
  inline?: boolean;
  monogramOnly?: boolean;
}) {
  if (kind === "genre")
    return (
      <GeneratedCover
        id={id}
        title={title}
        artist={artist}
        inline={inline}
        monogramOnly={monogramOnly}
      />
    );
  return (
    <div
      className="fallback-cover"
      role="img"
      aria-label={label(kind, title)}
      data-kind={kind}
      data-quiet=""
    >
      <Glyph kind={kind} />
    </div>
  );
}

const FallbackCover = memo(FallbackCoverView);

export default FallbackCover;
