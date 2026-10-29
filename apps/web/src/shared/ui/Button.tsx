import type { ComponentProps } from "react";
import { Check } from "lucide-react";

export type ButtonVariant =
  | "primary"
  | "action"
  | "secondary"
  | "destructive"
  | "active";

type ButtonProps = ComponentProps<"button"> & {
  variant?: ButtonVariant;
  pressed?: boolean;
};

export default function Button({
  variant = "secondary",
  pressed,
  className = "",
  type = "button",
  children,
  ...rest
}: ButtonProps) {
  const isActive = variant === "active";
  return (
    <button
      {...rest}
      type={type}
      className={`button ${variant} ${className}`.trim()}
      aria-pressed={isActive ? (pressed ?? true) : pressed}
    >
      {isActive && <Check size={15} strokeWidth={2.6} aria-hidden="true" />}
      {children}
    </button>
  );
}
