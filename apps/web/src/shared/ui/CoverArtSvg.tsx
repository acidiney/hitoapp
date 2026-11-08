import { memo, useId, type ReactNode } from "react";
import type { CoverArtSpec, CoverPaint } from "./coverArt";

const STAGE = { width: 100, height: 52 };
const HORIZON = 58;

const ALBUM_MOTIFS: ((paint: CoverPaint) => ReactNode)[] = [
  (p) => (
    <>
      <circle cx="100" cy="58" r="46" fill={p.accent} />
      <circle cx="18" cy="14" r="7" fill={p.accentSoft} />
    </>
  ),
  (p) => (
    <>
      <rect
        x="-20"
        y="24"
        width="140"
        height="14"
        fill={p.accent}
        transform="rotate(-24 50 29)"
      />
      <rect
        x="-20"
        y="46"
        width="140"
        height="5"
        fill={p.accentSoft}
        transform="rotate(-24 50 29)"
      />
    </>
  ),
  (p) => (
    <>
      <circle
        cx="70"
        cy="27"
        r="19"
        fill="none"
        stroke={p.accent}
        strokeWidth="9"
      />
      <circle cx="22" cy="40" r="6" fill={p.accentSoft} />
    </>
  ),
  (p) => (
    <>
      <path d="M0 58 L100 0 L100 58 Z" fill={p.accent} />
      <path d="M44 58 L100 24 L100 58 Z" fill={p.accentSoft} />
    </>
  ),
  (p) => (
    <>
      {[50, 63, 76, 89].map((x, index) => (
        <rect
          key={x}
          x={x}
          y="0"
          width="8"
          height="58"
          fill={index % 2 ? p.accentSoft : p.accent}
        />
      ))}
    </>
  ),
  (p) => (
    <>
      <circle cx="0" cy="0" r="46" fill={p.accent} />
      <circle cx="78" cy="32" r="9" fill={p.accentSoft} />
    </>
  ),
];

const ARTIST_DISCS = [
  [50, 32, 20],
  [34, 32, 18],
  [66, 32, 18],
  [50, 30, 24],
  [50, 34, 15],
  [50, 32, 22],
] as const;

function artistMotif(p: CoverPaint): ReactNode {
  const [cx, cy, r] = ARTIST_DISCS[p.motif];
  return (
    <>
      <circle cx={cx} cy={cy} r={r} fill={p.accent} />
      {p.motif === 4 && (
        <circle
          cx={cx}
          cy={cy}
          r={r + 9}
          fill="none"
          stroke={p.accentSoft}
          strokeWidth="3"
        />
      )}
      {p.motif % 3 === 1 && (
        <circle cx="72" cy="16" r="5" fill={p.accentSoft} />
      )}
      {p.motif === 2 && <circle cx="28" cy="16" r="5" fill={p.accentSoft} />}
    </>
  );
}

const PLAYLIST_WIDTHS = [
  [60, 44, 66, 34],
  [44, 66, 34, 60],
  [66, 34, 60, 44],
  [34, 60, 44, 66],
  [52, 66, 40, 58],
  [66, 50, 58, 36],
] as const;

function playlistMotif(p: CoverPaint): ReactNode {
  return (
    <>
      {PLAYLIST_WIDTHS[p.motif].map((width, index) => {
        const y = 7 + index * 12.5;
        const fill = index % 2 ? p.accentSoft : p.accent;
        return (
          <g key={index}>
            <circle cx="14" cy={y + 3.5} r="3.5" fill={fill} />
            <rect x="24" y={y} width={width} height="7" rx="3.5" fill={fill} />
          </g>
        );
      })}
    </>
  );
}

const PODCAST_CENTRES = [
  [84, 6],
  [16, 6],
  [88, 36],
  [12, 34],
  [50, -2],
  [96, -4],
] as const;

function podcastMotif(p: CoverPaint): ReactNode {
  const [cx, cy] = PODCAST_CENTRES[p.motif];
  return (
    <>
      {[14, 28, 42, 56].map((radius, index) => (
        <circle
          key={radius}
          cx={cx}
          cy={cy}
          r={radius}
          fill="none"
          stroke={index % 2 ? p.accentSoft : p.accent}
          strokeWidth="5"
        />
      ))}
      <circle cx={cx} cy={cy} r="5" fill={p.accent} />
    </>
  );
}

const AUDIOBOOK_LINES = [54, 60, 48, 58, 36] as const;

function audiobookMotif(p: CoverPaint): ReactNode {
  return (
    <>
      {AUDIOBOOK_LINES.map((_, index) => {
        const width =
          AUDIOBOOK_LINES[(index + p.motif) % AUDIOBOOK_LINES.length];
        return (
          <rect
            key={index}
            x="24"
            y={8 + index * 9.5}
            width={width}
            height="3.5"
            rx="1.75"
            fill={index % 2 ? p.accentSoft : p.accent}
          />
        );
      })}
    </>
  );
}

const MOTIFS: Record<CoverPaint["kind"], (paint: CoverPaint) => ReactNode> = {
  album: (p) => ALBUM_MOTIFS[p.motif](p),
  genre: (p) => ALBUM_MOTIFS[p.motif](p),
  artist: artistMotif,
  playlist: playlistMotif,
  podcast: podcastMotif,
  audiobook: audiobookMotif,
};

function orientationTransform(p: CoverPaint): string | undefined {
  const mirrorX = p.kind !== "audiobook" && (p.orientation & 1) === 1;
  const mirrorY = p.kind !== "artist" && (p.orientation & 2) === 2;
  if (!mirrorX && !mirrorY) return undefined;
  const parts: string[] = [];
  if (mirrorX) parts.push(`translate(${STAGE.width} 0) scale(-1 1)`);
  if (mirrorY) parts.push(`translate(0 ${STAGE.height}) scale(1 -1)`);
  return parts.join(" ");
}

function Foreground({ paint }: { paint: CoverPaint }) {
  if (paint.kind === "artist")
    return (
      <rect
        x="0"
        y={HORIZON}
        width="100"
        height={100 - HORIZON}
        fill={paint.deep}
      />
    );
  if (paint.kind === "audiobook")
    return (
      <>
        <rect
          x="0"
          y="0"
          width="10"
          height="100"
          fill={paint.motif % 2 ? paint.accentSoft : paint.accent}
        />
        <rect x="10" y="0" width="1.5" height="100" fill={paint.deep} />
      </>
    );
  return null;
}

function CoverArtView({ art }: { art: CoverArtSpec }) {
  const { paint } = art;
  const gradient = `cover-${useId().replace(/\W/g, "")}`;
  return (
    <svg
      className="cover-art"
      viewBox="0 0 100 100"
      preserveAspectRatio="xMidYMid slice"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient id={gradient} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={paint.base} />
          <stop offset="1" stopColor={paint.deep} />
        </linearGradient>
      </defs>
      <rect width="100" height="100" fill={`url(#${gradient})`} />
      <svg
        x="0"
        y="0"
        width={STAGE.width}
        height={STAGE.height}
        viewBox={`0 0 ${STAGE.width} ${STAGE.height}`}
      >
        <g className="cover-art-motif" transform={orientationTransform(paint)}>
          {MOTIFS[paint.kind](paint)}
        </g>
      </svg>
      <Foreground paint={paint} />
    </svg>
  );
}

const CoverArtSvg = memo(
  CoverArtView,
  (previous, next) => previous.art.paint === next.art.paint,
);

export default CoverArtSvg;
