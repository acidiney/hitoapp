import {
  useId,
  useState,
  type ChangeEvent,
  type ComponentProps,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { Eye, EyeOff } from "lucide-react";

export type DotStatus = "ready" | "checking" | "failed";

export function AuthDot({ status }: { status: DotStatus }) {
  return <span className="auth-dot" data-status={status} aria-hidden="true" />;
}

export function ServerLine({ busy }: { busy: boolean }) {
  return (
    <span className="auth-server">
      <AuthDot status={busy ? "checking" : "ready"} />
      <span>
        {window.location.host}
        {busy ? " · signing in…" : ""}
      </span>
    </span>
  );
}

export function AuthAlert({ children }: { children: ReactNode }) {
  return (
    <p className="auth-alert" role="alert">
      <AuthDot status="failed" />
      <span>{children}</span>
    </p>
  );
}

export function DotSpinner() {
  return (
    <svg
      className="auth-spinner"
      aria-hidden="true"
      width="16"
      height="16"
      viewBox="0 0 24 24"
    >
      {[
        [19.85, 15.25],
        [15.25, 19.85],
        [8.75, 19.85],
        [4.15, 15.25],
        [4.15, 8.75],
        [8.75, 4.15],
        [15.25, 4.15],
        [19.85, 8.75],
      ].map(([cx, cy]) => (
        <circle
          key={`${cx}-${cy}`}
          cx={cx}
          cy={cy}
          r="1.5"
          fill="currentColor"
        />
      ))}
    </svg>
  );
}

type FieldProps = Omit<ComponentProps<"input">, "onChange" | "value"> & {
  label: string;
  value: string;
  onValue: (value: string) => void;
  hint?: ReactNode;
  hintId?: string;
  hintTone?: "muted" | "warning";
  pin?: boolean;
  reveal?: boolean;
};

export function AuthField({
  label,
  id: givenId,
  value,
  onValue,
  hint,
  hintId,
  hintTone = "muted",
  pin = false,
  reveal = false,
  type = "text",
  ...input
}: FieldProps) {
  const generatedId = useId();
  const id = givenId ?? generatedId;
  const [shown, setShown] = useState(false);
  const [capsLock, setCapsLock] = useState(false);
  const watchCaps = (event: KeyboardEvent<HTMLInputElement>) =>
    setCapsLock(event.getModifierState("CapsLock"));
  const describedBy = [input["aria-describedby"], hintId]
    .filter(Boolean)
    .join(" ");
  return (
    <div className="auth-field">
      <label htmlFor={id}>{label}</label>
      <span className="auth-control" data-pin={pin || undefined}>
        <input
          {...input}
          id={id}
          type={reveal && shown ? "text" : type}
          value={value}
          aria-describedby={describedBy || undefined}
          onChange={(event: ChangeEvent<HTMLInputElement>) =>
            onValue(event.target.value)
          }
          onKeyDown={reveal ? watchCaps : undefined}
          onKeyUp={reveal ? watchCaps : undefined}
          onBlur={reveal ? () => setCapsLock(false) : undefined}
        />
        {reveal && (
          <button
            type="button"
            className="auth-reveal"
            aria-label="Show password"
            aria-pressed={shown}
            onClick={() => setShown((current) => !current)}
          >
            {shown ? (
              <EyeOff size={18} aria-hidden="true" />
            ) : (
              <Eye size={18} aria-hidden="true" />
            )}
          </button>
        )}
      </span>
      {capsLock && (
        <span className="auth-hint" data-tone="warning">
          Caps Lock is on
        </span>
      )}
      {hint && (
        <span id={hintId} className="auth-hint" data-tone={hintTone}>
          {hint}
        </span>
      )}
    </div>
  );
}
