export type SourceStatus = "ready" | "checking" | "none" | "failed";

const WORDS: Record<SourceStatus, string> = {
  ready: "Ready",
  checking: "Checking…",
  none: "No source",
  failed: "Couldn't check",
};

export function sourceStatusWord(status: SourceStatus): string {
  return WORDS[status];
}

export default function StatusDot({
  status,
  detail,
  as: Tag = "span",
  role,
}: {
  status: SourceStatus;
  detail?: string;
  as?: "span" | "small";
  role?: "status";
}) {
  return (
    <Tag className="source-status" data-status={status} role={role}>
      <span className="source-status-dot" aria-hidden="true" />
      {detail ? `${WORDS[status]} · ${detail}` : WORDS[status]}
    </Tag>
  );
}
