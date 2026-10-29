import type { CSSProperties, ReactNode, Ref } from "react";

export type Crumb = { label: string; href?: string };

export default function CollectionHeader({
  tint,
  size,
  eyebrow,
  title,
  stats,
  cover,
  crumbs,
  actions,
  headingRef,
  children,
}: {
  tint: string;
  size: "artist" | "playlist";
  eyebrow: ReactNode;
  title: string;
  stats: ReactNode;
  cover?: ReactNode;
  crumbs?: ReactNode;
  actions?: ReactNode;
  headingRef?: Ref<HTMLHeadingElement>;
  children?: ReactNode;
}) {
  return (
    <header
      className="tinted-header"
      data-size={size}
      style={{ "--tint": tint } as CSSProperties}
    >
      {crumbs}
      <div className="tinted-body">
        {cover && <div className="tinted-cover">{cover}</div>}
        <div className="tinted-copy">
          <span className="tinted-eyebrow">{eyebrow}</span>
          <h1 ref={headingRef}>{title}</h1>
          <span className="tinted-stats">{stats}</span>
          {size === "playlist" && actions && (
            <div className="tinted-actions">{actions}</div>
          )}
        </div>
        {size === "artist" && actions && (
          <div className="tinted-actions">{actions}</div>
        )}
      </div>
      {children}
    </header>
  );
}
