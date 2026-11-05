import type { ReactNode } from "react";

export default function SettingRow({
  title,
  hint,
  leading,
  status,
  action,
  wide = false,
  compact = false,
  children,
}: {
  title: ReactNode;
  hint?: ReactNode;
  leading?: ReactNode;
  status?: ReactNode;
  action?: ReactNode;
  wide?: boolean;
  compact?: boolean;
  children?: ReactNode;
}) {
  const copy = (
    <div className="setting-line-copy">
      <div className="setting-line-title">{title}</div>
      {hint && <div className="setting-line-hint">{hint}</div>}
    </div>
  );
  return (
    <div
      className="setting-line"
      data-wide={wide || undefined}
      data-compact={compact || undefined}
    >
      <div className="setting-line-main">
        {leading ? (
          <div className="setting-line-identity">
            {leading}
            {copy}
          </div>
        ) : (
          copy
        )}
        {!compact && <div className="setting-line-status">{status}</div>}
        <div className="setting-line-action">{action}</div>
      </div>
      {children}
    </div>
  );
}
