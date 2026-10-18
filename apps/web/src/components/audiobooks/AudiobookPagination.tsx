import { Link } from "@inertiajs/react";
import type { ComponentProps } from "react";

export default function AudiobookPagination({
  nextCursor,
  previousCursor,
  limit,
  pending,
  failedHref,
  error,
  linkProps,
}: {
  nextCursor: string | null;
  previousCursor: string | null;
  limit: number;
  pending: boolean;
  failedHref: string | null;
  error: string;
  linkProps: (href: string) => Partial<ComponentProps<typeof Link>>;
}) {
  const first = limit === 60 ? "/audiobooks" : `/audiobooks?limit=${limit}`;
  const page = (cursor: string) =>
    `${first}${first.includes("?") ? "&" : "?"}cursor=${encodeURIComponent(cursor)}`;
  if (!nextCursor && !previousCursor && !error) return null;
  return (
    <nav
      className="pagination"
      aria-label="Audiobook pages"
      aria-busy={pending}
    >
      {(previousCursor || error) && (
        <Link className="button" href={first} {...linkProps(first)}>
          First audiobooks
        </Link>
      )}
      {previousCursor && (
        <Link
          className="button"
          href={page(previousCursor)}
          {...linkProps(page(previousCursor))}
        >
          Previous audiobooks
        </Link>
      )}
      {nextCursor && (
        <Link
          className="button"
          href={page(nextCursor)}
          {...linkProps(page(nextCursor))}
        >
          Next audiobooks
        </Link>
      )}
      {pending && <p role="status">Loading audiobooks…</p>}
      {error && <p role="alert">{error}</p>}
      {failedHref && (
        <Link className="button" href={failedHref} {...linkProps(failedHref)}>
          Retry audiobook page
        </Link>
      )}
    </nav>
  );
}
