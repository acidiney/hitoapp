import type { Schema } from "../../../shared/schema";

export function canonicalContentUrl(value: string): string | null {
  if (value.length > 2048 || /[\s\u0000-\u001f\u007f-\u009f\\?#]/u.test(value))
    return null;
  try {
    const url = new URL(value);
    return url.protocol === "https:" &&
      url.hostname !== "" &&
      url.username === "" &&
      url.password === "" &&
      url.href === value
      ? value
      : null;
  } catch {
    return null;
  }
}

export default function RecordingCredits({
  attribution,
}: {
  attribution?: Schema<"RecordingAttribution"> | null;
}) {
  if (!attribution) return null;
  const href = canonicalContentUrl(attribution.content_url);
  return (
    <dl className="metadata-tag-list recording-credits">
      <div>
        <dt>Creators</dt>
        <dd>
          <ol aria-label="Creators">
            {attribution.creators.map((creator, index) => (
              <li key={index}>{creator}</li>
            ))}
          </ol>
        </dd>
      </div>
      <div>
        <dt>Provider</dt>
        <dd>
          {href ? (
            <a
              href={href}
              target="_blank"
              rel="noopener noreferrer"
              referrerPolicy="no-referrer"
            >
              {attribution.provider_label}
            </a>
          ) : (
            attribution.provider_label
          )}
        </dd>
      </div>
    </dl>
  );
}
