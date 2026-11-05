import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { Check, ChevronDown } from "lucide-react";

export type ListboxOption = { value: string; label: string };

const typeaheadWindowMs = 600;

function moved(key: string, active: number, count: number): number | null {
  if (key === "ArrowDown") return Math.min(active + 1, count - 1);
  if (key === "ArrowUp") return Math.max(active - 1, 0);
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  return null;
}

function typed(
  options: ListboxOption[],
  active: number,
  search: string,
): number | null {
  const needle = search.toLowerCase();
  const cycling = [...needle].every((letter) => letter === needle[0]);
  const prefix = cycling ? needle[0] : needle;
  const start = cycling ? active + 1 : active;
  for (let step = 0; step < options.length; step += 1) {
    const index = (start + step) % options.length;
    if (options[index].label.toLowerCase().startsWith(prefix)) return index;
  }
  return null;
}

export default function Listbox({
  label,
  options,
  value,
  onChange,
  disabled = false,
}: {
  label: string;
  options: ListboxOption[];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  const id = useId();
  const labelId = `${id}-label`;
  const listId = `${id}-list`;
  const optionId = (index: number) => `${id}-option-${index}`;
  const found = options.findIndex((option) => option.value === value);
  const current = Math.max(found, 0);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(current);
  const search = useRef({ text: "", at: 0 });
  const list = useRef<HTMLUListElement>(null);
  const expanded = open && !disabled && options.length > 0;

  useEffect(() => {
    if (!expanded) return;
    list.current
      ?.querySelector(`[id="${optionId(active)}"]`)
      ?.scrollIntoView?.({ block: "nearest" });
  });

  function show(index = current) {
    setActive(index);
    setOpen(true);
  }

  function commit(index: number) {
    setOpen(false);
    if (options[index].value !== value) onChange(options[index].value);
  }

  function press(event: KeyboardEvent<HTMLButtonElement>) {
    if (disabled || event.altKey || event.ctrlKey || event.metaKey) return;
    const { key } = event;
    if (key === "Tab") {
      if (expanded) commit(active);
      return;
    }
    if (key === "Escape") {
      if (!expanded) return;
      event.preventDefault();
      setOpen(false);
      return;
    }
    if (key === "Enter" || key === " ") {
      event.preventDefault();
      if (expanded) commit(active);
      else show();
      return;
    }
    const step = moved(key, expanded ? active : current, options.length);
    if (step !== null) {
      event.preventDefault();
      if (expanded) setActive(step);
      else show(key === "Home" || key === "End" ? step : current);
      return;
    }
    if (key.length !== 1) return;
    event.preventDefault();
    const now = Date.now();
    const text =
      now - search.current.at > typeaheadWindowMs
        ? key
        : search.current.text + key;
    search.current = { text, at: now };
    const match = typed(options, expanded ? active : current, text);
    if (match !== null) show(match);
  }

  return (
    <div className="listbox">
      <span id={labelId} className="listbox-label">
        {label}
      </span>
      <button
        type="button"
        role="combobox"
        className="listbox-control"
        aria-haspopup="listbox"
        aria-expanded={expanded}
        aria-controls={listId}
        aria-labelledby={labelId}
        aria-activedescendant={expanded ? optionId(active) : undefined}
        disabled={disabled}
        onClick={(event) => {
          event.currentTarget.focus();
          if (expanded) setOpen(false);
          else show();
        }}
        onKeyDown={press}
        onBlur={() => setOpen(false)}
      >
        <span>{options[current]?.label}</span>
        <ChevronDown size={18} aria-hidden="true" />
      </button>
      {expanded && (
        <ul
          ref={list}
          id={listId}
          role="listbox"
          aria-labelledby={labelId}
          className="listbox-options"
          onMouseDown={(event) => event.preventDefault()}
        >
          {options.map((option, index) => (
            <li
              key={option.value}
              id={optionId(index)}
              role="option"
              aria-selected={index === current}
              data-active={index === active}
              onMouseMove={() => setActive(index)}
              onClick={() => commit(index)}
            >
              <span>{option.label}</span>
              {index === current && <Check size={16} aria-hidden="true" />}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
