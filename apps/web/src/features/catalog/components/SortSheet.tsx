import { useState } from "react";
import { SlidersHorizontal, X } from "lucide-react";
import Dialog from "../../../shared/ui/Dialog";
import type { ListboxOption } from "../../../shared/ui/Listbox";

type Choice = {
  options: ListboxOption[];
  value: string;
  onChange: (value: string) => void;
};

function Chips({
  label,
  title,
  choice,
}: {
  label: string;
  title?: string;
  choice: Choice;
}) {
  const group = (
    <div className="sheet-group" role="group" aria-label={label}>
      {choice.options.map((option) => (
        <button
          key={option.value}
          type="button"
          className="sheet-chip"
          aria-pressed={option.value === choice.value}
          onClick={() => choice.onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
  if (!title) return group;
  return (
    <section className="sheet-section">
      <p className="sheet-label" aria-hidden="true">
        {title}
      </p>
      {group}
    </section>
  );
}

export default function SortSheet({
  options,
  value,
  onChange,
  filter,
}: Choice & { filter?: Choice }) {
  const [open, setOpen] = useState(false);
  const current = options.find((option) => option.value === value);
  const filtered = filter?.options.find(
    (option) => option.value === filter.value && option !== filter.options[0],
  );
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
        <span>
          Sort · {current?.label ?? options[0]?.label}
          {filtered && ` · ${filtered.label}`}
        </span>
      </button>
      {open && (
        <Dialog
          title={filter ? "Filter and sort" : "Sort tracks"}
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
          {filter && (
            <Chips label="Availability" title="Availability" choice={filter} />
          )}
          <Chips
            label="Sort by"
            title={filter ? "Sort" : undefined}
            choice={{ options, value, onChange }}
          />
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
