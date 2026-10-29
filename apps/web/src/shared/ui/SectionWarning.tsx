import type { ReactNode } from "react";

export default function SectionWarning({
  tone = "warning",
  title,
  children,
}: {
  tone?: "warning" | "error";
  title: string;
  children?: ReactNode;
}) {
  return (
    <div
      className="section-warning"
      data-tone={tone}
      role={tone === "error" ? "alert" : "status"}
    >
      <span className="section-warning-icon" aria-hidden="true">
        !
      </span>
      <span>
        <strong>{title}</strong> {children}
      </span>
    </div>
  );
}
