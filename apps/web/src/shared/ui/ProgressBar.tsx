export default function ProgressBar({
  percent,
  label,
}: {
  percent: number;
  label: string;
}) {
  const value = Math.min(100, Math.max(0, Math.round(percent)));
  return (
    <div
      className="progress-bar"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value}
    >
      <div className="progress-fill" style={{ width: `${value}%` }} />
    </div>
  );
}
