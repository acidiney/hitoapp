import type { Choice } from "./SegmentedControl";

export default function FilterPills<T extends string>({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: Choice<T>[];
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div className="pills" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={option.value === value}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
