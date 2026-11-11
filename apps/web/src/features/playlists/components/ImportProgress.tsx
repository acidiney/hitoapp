export default function ImportProgress({
  kind,
  service,
}: {
  kind: "copy" | "follow";
  service: string;
}) {
  return (
    <div className="import-progress">
      <div className="import-preview">
        <span className="import-preview-cover" aria-hidden="true" />
        <span>
          <strong>{kind === "copy" ? "Importing a copy" : "Following"}</strong>
          <small>{service} playlist</small>
        </span>
      </div>
      <div
        role="progressbar"
        aria-label={`Reading the playlist from ${service}`}
        className="import-progress-bar"
      >
        <span />
      </div>
      <p role="status" aria-live="polite">
        Reading the playlist from {service}. This can take up to a minute.
      </p>
    </div>
  );
}
