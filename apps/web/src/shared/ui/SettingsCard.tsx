import { useId, type ReactNode } from "react";

export default function SettingsCard({
  title,
  action,
  children,
}: {
  title: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section className="settings-card" aria-labelledby={id}>
      <header>
        <h2 id={id}>{title}</h2>
        {action}
      </header>
      <div className="settings-card-body">{children}</div>
    </section>
  );
}
