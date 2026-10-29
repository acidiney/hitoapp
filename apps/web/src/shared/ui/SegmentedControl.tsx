import { useRef, type KeyboardEvent } from "react";

export type Choice<T extends string> = { value: T; label: string };

function step(key: string, index: number, count: number): number | null {
  if (key === "ArrowRight" || key === "ArrowDown") return (index + 1) % count;
  if (key === "ArrowLeft" || key === "ArrowUp")
    return (index + count - 1) % count;
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  return null;
}

export default function SegmentedControl<T extends string>({
  label,
  options,
  value,
  onChange,
  disabled = false,
}: {
  label: string;
  options: Choice<T>[];
  value: T;
  onChange: (value: T) => void;
  disabled?: boolean;
}) {
  const selected = options.some((option) => option.value === value);
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  function press(event: KeyboardEvent, index: number) {
    const next = step(event.key, index, options.length);
    if (next === null) return;
    event.preventDefault();
    onChange(options[next].value);
    buttons.current[next]?.focus();
  }
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((option, index) => {
        const checked = option.value === value;
        const tabbable = selected ? checked : index === 0;
        return (
          <button
            key={option.value}
            ref={(element) => {
              buttons.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            tabIndex={tabbable ? 0 : -1}
            disabled={disabled}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => press(event, index)}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
