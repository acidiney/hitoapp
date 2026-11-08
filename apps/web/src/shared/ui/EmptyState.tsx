import type { ReactNode } from "react";
import { Link } from "@inertiajs/react";
import { Music2 } from "lucide-react";

export default function EmptyState({
  title,
  text,
  libraryLink = true,
  children,
}: {
  title: string;
  text: string;
  libraryLink?: boolean;
  children?: ReactNode;
}) {
  return (
    <div className="empty">
      <Music2 size={40} strokeWidth={1.3} />
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
