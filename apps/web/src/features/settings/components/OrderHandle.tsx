import { GripVertical } from "lucide-react";

import ActionMenu, { type ActionMenuItem } from "../../../shared/ui/ActionMenu";

export default function OrderHandle({
  label,
  position,
  items,
}: {
  label: string;
  position: number;
  items: ActionMenuItem[];
}) {
  return (
    <>
      <ActionMenu
        label={`Move ${label}`}
        triggerClassName="grip-button"
        icon={<GripVertical size={18} aria-hidden="true" />}
        items={items}
      />
      <span className="priority-number" aria-hidden="true">
        {position}
      </span>
    </>
  );
}
