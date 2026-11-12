import { useEffect, useState } from "react";

const SHOW_AFTER_MS = 300;

export default function Skeleton({ rows = 3 }: { rows?: number }) {
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    const timer = window.setTimeout(() => setVisible(true), SHOW_AFTER_MS);
    return () => window.clearTimeout(timer);
  }, []);
  if (!visible) return null;
  return (
    <div className="skeleton" aria-hidden="true">
      {Array.from({ length: rows }, (_, index) => (
        <div className="skeleton-row" key={index}>
          <span className="skeleton-cover" />
          <span className="skeleton-lines">
            <span />
            <span />
          </span>
          <span className="skeleton-time" />
        </div>
      ))}
    </div>
  );
}
