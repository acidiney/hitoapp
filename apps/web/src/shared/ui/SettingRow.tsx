import type { ReactNode } from "react";

export default function SettingRow({
  title,
  hint,
  status,
  action,
  wide = false,
  compact = false,
  children,
}: {
  title: ReactNode;
  hint?: ReactNode;
  status?: ReactNode;
  action?: ReactNode;
  wide?: boolean;
  compact?: boolean;
  children?: ReactNode;
}) {
  return (
    <div
      className="setting-line"
      data-wide={wide || undefined}
      data-compact={compact || undefined}
    >
      <div className="setting-line-main">
        <div className="setting-line-copy">
          <div className="setting-line-title">{title}</div>
          {hint && <div className="setting-line-hint">{hint}</div>}
        </div>
        {!compact && <div className="setting-line-status">{status}</div>}
        <div className="setting-line-action">{action}</div>
      </div>
      {children}
    </div>
  );
}
