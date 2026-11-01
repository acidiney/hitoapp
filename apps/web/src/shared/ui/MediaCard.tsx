import type { ReactNode } from "react";
import { Link } from "@inertiajs/react";
import { Check, Play } from "lucide-react";

type Heading = "h3" | "h4" | "h5";

export default function MediaCard({
  title,
  subtitle,
  cover,
  href,
  onOpen,
  openLabel,
  onPlay,
  playLabel,
  inLibrary,
  heading: HeadingTag,
  round = false,
  disabled = false,
}: {
  title: string;
  subtitle?: string;
  cover: ReactNode;
  href?: string;
  onOpen?: () => void;
  openLabel?: string;
  onPlay?: () => void;
  playLabel?: string;
  inLibrary?: string;
  heading?: Heading;
  round?: boolean;
  disabled?: boolean;
}) {
  const body = (
    <>
      <span className="media-card-cover">{cover}</span>
      {HeadingTag ? (
        <HeadingTag className="media-card-title">{title}</HeadingTag>
      ) : (
        <span className="media-card-title">{title}</span>
      )}
      {subtitle && <span className="media-card-subtitle">{subtitle}</span>}
    </>
  );
  return (
    <div className="media-card" data-round={round || undefined}>
      {href !== undefined ? (
        <Link className="media-card-open" href={href} aria-label={openLabel}>
          {body}
        </Link>
      ) : (
        <button
          type="button"
          className="media-card-open"
          aria-label={openLabel}
          disabled={disabled}
          onClick={onOpen}
        >
          {body}
        </button>
      )}
      <div className="media-card-overlay">
        {inLibrary && (
          <span className="media-card-owned" role="img" aria-label={inLibrary}>
            <Check size={12} strokeWidth={3} aria-hidden="true" />
          </span>
        )}
        {onPlay && (
          <button
            type="button"
            className="media-card-play"
            aria-label={playLabel ?? `Play ${title}`}
            disabled={disabled}
            onClick={onPlay}
          >
            <Play size={18} fill="currentColor" aria-hidden="true" />
          </button>
        )}
      </div>
    </div>
  );
}
