import { router } from "@inertiajs/react";
import { log } from "../log";

export default function Pagination({
  page,
  query,
  path,
  parameter = "offset",
  label = "Result pages",
}: {
  page: { total: number; offset: number; limit: number };
  query: string;
  path: string;
  parameter?: "offset" | "recording_offset";
  label?: string;
}) {
  if (page.total <= page.limit && page.offset === 0) return null;
  function visit(offset: number) {
    log.info("navigation.pagination", { route: path, index: offset });
    const parameters = Object.fromEntries(
      new URLSearchParams(window.location.search),
    );
    if (!Object.hasOwn(parameters, "q") && query) parameters.q = query;
    router.get(path, { ...parameters, [parameter]: offset });
  }
  return (
    <nav className="pagination" aria-label={label}>
      <button
        className="button"
        disabled={page.offset === 0}
        onClick={() => visit(Math.max(0, page.offset - page.limit))}
      >
        Previous
      </button>
      <span>
        {Math.floor(page.offset / page.limit) + 1} /{" "}
        {Math.ceil(page.total / page.limit)}
      </span>
      <button
        className="button"
        disabled={page.offset + page.limit >= page.total}
        onClick={() => visit(page.offset + page.limit)}
      >
        Next
      </button>
    </nav>
  );
}
