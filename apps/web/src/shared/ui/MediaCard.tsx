import type { ReactNode } from "react";
import { Link } from "@inertiajs/react";
import { Check, Play } from "lucide-react";

type Heading = "h3" | "h4" | "h5";

export default function MediaCard({
  title,
  subtitle,
  detail,
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
  detail?: ReactNode;
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
      <span className="shelf-card-cover">{cover}</span>
      {HeadingTag ? (
        <HeadingTag className="shelf-card-title">{title}</HeadingTag>
      ) : (
        <span className="shelf-card-title">{title}</span>
      )}
      {subtitle && <span className="shelf-card-subtitle">{subtitle}</span>}
      {detail}
    </>
  );
  return (
    <div className="shelf-card" data-round={round || undefined}>
      {href !== undefined ? (
        <Link className="shelf-card-open" href={href} aria-label={openLabel}>
          {body}
        </Link>
      ) : (
        <button
          type="button"
          className="shelf-card-open"
          aria-label={openLabel}
          disabled={disabled}
          onClick={onOpen}
        >
          {body}
        </button>
      )}
      <div className="shelf-card-overlay">
        {inLibrary && (
          <span className="shelf-card-owned" role="img" aria-label={inLibrary}>
            <Check size={12} strokeWidth={3} aria-hidden="true" />
          </span>
        )}
        {onPlay && (
          <button
            type="button"
            className="shelf-card-play"
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
