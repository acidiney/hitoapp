import type { ReactNode } from "react";

export default function ListRow({
  cover,
  title,
  subtitle,
  state,
  action,
}: {
  cover: ReactNode;
  title: ReactNode;
  subtitle?: ReactNode;
  state?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="list-row">
      <div className="list-row-cover">{cover}</div>
      <span className="list-row-copy">
        <span className="list-row-title">{title}</span>
        {subtitle && <span className="list-row-subtitle">{subtitle}</span>}
      </span>
      <span className="list-row-state">{state}</span>
      <span className="list-row-action">{action}</span>
    </div>
  );
}
