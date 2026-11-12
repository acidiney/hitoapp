import type { ReactNode } from "react";

export default function DiscographyState({
  title,
  text,
  role,
  hidden,
  children,
}: {
  title: string;
  text?: ReactNode;
  role?: "status" | "alert";
  hidden?: string;
  children?: ReactNode;
}) {
  return (
    <div className="state-card">
      <span className="state-card-title" role={role}>
        {title}
        {hidden && <span className="sr-only">{hidden}</span>}
      </span>
      {text && <span className="state-card-text">{text}</span>}
      {children}
    </div>
  );
}
