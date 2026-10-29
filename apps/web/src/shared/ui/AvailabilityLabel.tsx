export type Availability = "library" | "available" | "catalog";

const WORDS: Record<Availability, string> = {
  library: "In library",
  available: "Available",
  catalog: "Catalog only",
};

export default function AvailabilityLabel({
  state,
  label,
}: {
  state: Availability;
  label?: string;
}) {
  return (
    <span className="availability" data-state={state}>
      <span className="ring" data-ring={state} aria-hidden="true" />
      {label ?? WORDS[state]}
    </span>
  );
}
