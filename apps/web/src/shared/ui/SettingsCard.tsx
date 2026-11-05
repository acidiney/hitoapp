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
  const hint = description ?? setting.description;
  return (
    <section
      id={`setting-${setting.id}`}
      className="settings-card"
      aria-labelledby={headingId}
      aria-busy={busy}
    >
      <header>
        <div>
          <h3 id={headingId} ref={headingRef} tabIndex={-1}>
            {setting.label}
          </h3>
          {hint && <p className="settings-card-hint">{hint}</p>}
        </div>
        {action}
      </header>
      <div className="settings-card-body">{children}</div>
    </section>
  );
}
