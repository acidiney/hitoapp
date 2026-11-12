import { useId } from "react";

export default function Switch({
  id,
  label,
  hint,
  checked,
  onChange,
  disabled = false,
}: {
  id?: string;
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  const key = useId();
  return (
    <div
      id={id && `setting-${id}`}
      tabIndex={id ? -1 : undefined}
      className="switch-row"
    >
      <span className="switch-copy">
        <span id={`${key}-label`} className="switch-label">
          {label}
        </span>
        {hint && (
          <span id={`${key}-hint`} className="switch-hint">
            {hint}
          </span>
        )}
      </span>
      <button
        type="button"
        role="switch"
        className="switch"
        aria-checked={checked}
        aria-labelledby={`${key}-label`}
        aria-describedby={hint ? `${key}-hint` : undefined}
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className="switch-knob" aria-hidden="true" />
      </button>
    </div>
  );
}
