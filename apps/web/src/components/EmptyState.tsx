import { Link } from "@inertiajs/react";
import { Music2 } from "lucide-react";

export default function EmptyState({
  title,
  text,
  libraryLink = true,
}: {
  title: string;
  text: string;
  libraryLink?: boolean;
}) {
  return (
    <div className="empty">
      <Music2 size={40} strokeWidth={1.3} />
      <h2>{title}</h2>
      <p>{text}</p>
      {libraryLink && (
        <Link className="text-link" href="/library">
          Go to library
        </Link>
      )}
    </div>
  );
}
