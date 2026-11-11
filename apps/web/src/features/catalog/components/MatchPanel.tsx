import { useId, type ReactNode } from "react";

export default function MatchPanel({
  title,
  busy = false,
  children,
}: {
  title: string;
  busy?: boolean;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section className="match-panel" aria-labelledby={id} aria-busy={busy}>
      <h3 id={id}>{title}</h3>
      {children}
    </section>
  );
}
