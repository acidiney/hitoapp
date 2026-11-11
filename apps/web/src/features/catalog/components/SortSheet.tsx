import { useState } from "react";
import { SlidersHorizontal, X } from "lucide-react";
import Dialog from "../../../shared/ui/Dialog";
import type { ListboxOption } from "../../../shared/ui/Listbox";

export default function SortSheet({
  options,
  value,
  onChange,
}: {
  options: ListboxOption[];
  value: string;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const current = options.find((option) => option.value === value);
  return (
    <>
      <button
        type="button"
        className="sheet-trigger"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen(true)}
      >
        <SlidersHorizontal size={16} aria-hidden="true" />
        <span>Sort · {current?.label ?? options[0]?.label}</span>
      </button>
      {open && (
        <Dialog
          title="Sort tracks"
          className="sort-sheet"
          onClose={() => setOpen(false)}
        >
          <button
            type="button"
            className="sheet-close"
            aria-label="Close"
            onClick={() => setOpen(false)}
          >
            <X size={18} aria-hidden="true" />
          </button>
          <div className="sheet-group" role="group" aria-label="Sort by">
            {options.map((option) => (
              <button
                key={option.value}
                type="button"
                className="sheet-chip"
                aria-pressed={option.value === value}
                onClick={() => onChange(option.value)}
              >
                {option.label}
              </button>
            ))}
          </div>
          <button
            type="button"
            className="button primary sheet-done"
            onClick={() => setOpen(false)}
          >
            Done
          </button>
        </Dialog>
      )}
    </>
  );
}
