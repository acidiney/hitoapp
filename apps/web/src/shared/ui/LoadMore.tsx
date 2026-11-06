import { useEffect, useRef } from "react";
import Button from "./Button";

export default function LoadMore({
  loaded,
  total,
  noun,
  hasMore,
  loading,
  error,
  onLoad,
}: {
  loaded: number;
  total: number;
  noun: string;
  hasMore: boolean;
  loading: boolean;
  error: string;
  onLoad(): void;
}) {
  const sentinel = useRef<HTMLDivElement>(null);
  const load = useRef(onLoad);
  load.current = onLoad;
  const waiting = hasMore && !loading && !error;
  useEffect(() => {
    const element = sentinel.current;
    if (!waiting || !element || typeof IntersectionObserver === "undefined")
      return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          observer.disconnect();
          load.current();
        }
      },
      { rootMargin: "600px 0px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [waiting, loaded]);
  if (!hasMore && !error) return null;
  return (
    <div className="load-more">
      <div ref={sentinel} className="load-more-sentinel" aria-hidden="true" />
      <p role="status" className="load-more-status">
        {loading
          ? `Loading more ${noun}…`
          : `Showing ${loaded} of ${total} ${noun}`}
      </p>
      {error && (
        <p role="alert" className="form-error">
          {error}
        </p>
      )}
      {error ? (
        <Button onClick={onLoad}>Try again</Button>
      ) : (
        <Button disabled={loading} onClick={onLoad}>
          Load more
        </Button>
      )}
    </div>
  );
}
