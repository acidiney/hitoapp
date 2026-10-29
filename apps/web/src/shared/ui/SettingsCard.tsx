import { useId, type ReactNode, type Ref } from "react";

export default function SettingsCard({
  title,
  description,
  action,
  headingRef,
  busy,
  children,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
  headingRef?: Ref<HTMLHeadingElement>;
  busy?: boolean;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section className="settings-card" aria-labelledby={id} aria-busy={busy}>
      <header>
        <div>
          <h3 id={id} ref={headingRef} tabIndex={headingRef ? -1 : undefined}>
            {title}
          </h3>
          {description && <p className="settings-card-hint">{description}</p>}
        </div>
        {action}
      </header>
      <div className="settings-card-body">{children}</div>
    </section>
  );
}
