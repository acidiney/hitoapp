import type { ReactNode } from "react";
import ActionMenu, { type ActionMenuItem } from "../../../shared/ui/ActionMenu";

function titleSize(name: string) {
  if (name.length > 100) return "xl";
  if (name.length > 48) return "long";
  if (name.length > 24) return "medium";
  return "short";
}

export default function ArtistHeader({
  variant,
  photo,
  name,
  stats,
  actions,
  more,
}: {
  variant: "credit" | "chosen";
  photo: ReactNode;
  name: string;
  stats: ReactNode;
  actions?: ReactNode;
  more?: { label: string; items: ActionMenuItem[] };
}) {
  return (
    <header className="artist-header" data-variant={variant}>
      <span className="artist-header-photo">{photo}</span>
      <div className="artist-header-copy">
        <span className="artist-header-eyebrow">Artist</span>
        <h1 title={name} data-size={titleSize(name)}>
          {name}
        </h1>
        <span className="artist-header-stats">{stats}</span>
        <div className="artist-header-actions">
          {actions}
          {more && (
            <ActionMenu
              label={more.label}
              items={more.items}
              triggerClassName="pill-icon"
            />
          )}
        </div>
      </div>
    </header>
  );
}
