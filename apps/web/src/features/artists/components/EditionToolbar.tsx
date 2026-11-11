import { ChevronDown } from "lucide-react";
import ActionMenu, { type ActionMenuItem } from "../../../shared/ui/ActionMenu";

export type EditionFilter = "all" | "library";

export default function EditionToolbar({
  name,
  total,
  owned,
  filter,
  onFilter,
  items,
}: {
  name: string;
  total: number;
  owned: number;
  filter: EditionFilter;
  onFilter: (filter: EditionFilter) => void;
  items: ActionMenuItem[];
}) {
  return (
    <div className="edition-toolbar">
      <button
        type="button"
        className="edition-chip"
        aria-pressed={filter === "all"}
        onClick={() => onFilter("all")}
      >
        All {total}
      </button>
      <button
        type="button"
        className="edition-chip"
        aria-pressed={filter === "library"}
        onClick={() => onFilter("library")}
      >
        In my library {owned}
      </button>
      {items.length > 0 && (
        <ActionMenu
          label={`Edition ${name}`}
          triggerClassName="edition-chip edition-chip-menu"
          items={items}
          icon={
            <>
              <span className="edition-chip-label">Edition</span>
              <span className="edition-chip-name">{name}</span>
              <ChevronDown size={14} aria-hidden="true" />
            </>
          }
        />
      )}
    </div>
  );
}
