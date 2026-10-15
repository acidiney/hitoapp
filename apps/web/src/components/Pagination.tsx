import { router } from "@inertiajs/react";
import type { RecordingPage } from "../types";
import { log } from "../log";

export default function Pagination({
  page,
  query,
  path,
}: {
  page: RecordingPage;
  query: string;
  path: string;
}) {
  if (page.total <= page.limit) return null;
  function visit(offset: number) {
    log.info("navigation.pagination", { route: path, index: offset });
    router.get(path, { q: query, offset });
  }
  return (
    <div className="pagination">
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
    </div>
  );
}
