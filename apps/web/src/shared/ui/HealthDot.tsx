export type HealthTone = "success" | "warning" | "action";

export default function HealthDot({
  tone,
  label,
}: {
  tone: HealthTone;
  label: string;
}) {
  return (
    <span className="health" data-tone={tone}>
      <span className="health-dot" aria-hidden="true" />
      {label}
    </span>
  );
}
