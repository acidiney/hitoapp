import { memo, useMemo, type CSSProperties } from "react";
import CoverArtSvg from "./CoverArtSvg";
import { coverArt, type CoverArtKind } from "./coverArt";

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
  kind?: CoverArtKind;
  inline?: boolean;
  monogramOnly?: boolean;
}) {
  const art = useMemo(
    () => coverArt(kind, id, title, artist),
    [kind, id, title, artist],
  );
  const style = { "--cover-base": art.paint.base } as CSSProperties;
  return (
    <div
      className="fallback-cover"
      role="img"
      aria-label={`${kind === "artist" ? "Photo" : "Cover"} of ${title}`}
      data-kind={art.paint.kind}
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

const FallbackCover = memo(FallbackCoverView);

export default FallbackCover;
