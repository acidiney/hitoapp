import type { ReactNode } from "react";
import { Link } from "@inertiajs/react";

export type EmptyMotif = "first-run" | "no-match" | "unreachable";

export default function EmptyState({
  title,
  text,
  libraryLink = true,
  motif = "first-run",
  children,
}: {
  title: string;
  text: string;
  libraryLink?: boolean;
  motif?: EmptyMotif;
  children?: ReactNode;
}) {
  return (
    <div className="empty">
      <span className="empty-motif" data-motif={motif} aria-hidden="true">
        <span />
        <span />
        <span />
      </span>
      <h2>{title}</h2>
      <p>{text}</p>
      {children}
      {libraryLink && (
        <Link className="button" href="/library">
          Go to library
        </Link>
      )}
    </div>
  );
}
