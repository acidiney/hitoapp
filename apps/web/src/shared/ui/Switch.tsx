import { useId } from "react";

export default function Switch({
  label,
  hint,
  checked,
  onChange,
  disabled = false,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="switch-row">
      <span className="switch-copy">
        <span id={`${id}-label`} className="switch-label">
          {label}
        </span>
        {hint && (
          <span id={`${id}-hint`} className="switch-hint">
            {hint}
          </span>
        )}
      </span>
      <button
        type="button"
        role="switch"
        className="switch"
        aria-checked={checked}
        aria-labelledby={`${id}-label`}
        aria-describedby={hint ? `${id}-hint` : undefined}
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className="switch-knob" aria-hidden="true" />
      </button>
    </div>
  );
}
