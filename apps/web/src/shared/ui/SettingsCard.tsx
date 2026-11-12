import { useId, type ReactNode, type Ref } from "react";

export type SettingsCardIdentity = {
  id: string;
  label: string;
  description?: string;
};

export default function SettingsCard({
  setting,
  description,
  action,
  headingRef,
  busy,
  children,
}: {
  setting: SettingsCardIdentity;
  description?: string;
  action?: ReactNode;
  headingRef?: Ref<HTMLHeadingElement>;
  busy?: boolean;
  children: ReactNode;
}) {
  const headingId = useId();
  return (
    <section
      id={`setting-${setting.id}`}
      className="settings-card"
      aria-labelledby={headingId}
      aria-busy={busy}
    >
      <header>
        <h3 id={headingId} ref={headingRef} tabIndex={-1}>
          {setting.label}
        </h3>
        {action}
      </header>
      {description && <p className="settings-card-hint">{description}</p>}
      <div className="settings-card-body">{children}</div>
    </section>
  );
}
