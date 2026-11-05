import { useState, type CSSProperties } from "react";
import { avatarColours, initials } from "./avatarIdentity";

export default function Avatar({
  id,
  name,
  version,
  large = false,
}: {
  id: string;
  name: string;
  version?: string | null;
  large?: boolean;
}) {
  const colours = avatarColours(id);
  const style: CSSProperties = {
    background: colours.background,
    color: colours.foreground,
  };
  const source = version
    ? `/api/v1/accounts/${encodeURIComponent(id)}/avatar?v=${encodeURIComponent(version)}`
    : null;
  const [failed, setFailed] = useState<string | null>(null);
  const photo = source !== null && failed !== source;
  return (
    <span
      className={large ? "avatar large" : "avatar"}
      style={style}
      aria-hidden={photo ? undefined : "true"}
    >
      {photo ? (
        <img src={source} alt={name} onError={() => setFailed(source)} />
      ) : (
        initials(name)
      )}
    </span>
  );
}
