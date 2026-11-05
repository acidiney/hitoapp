import type { CSSProperties } from "react";
import { avatarColours, initials } from "./avatarIdentity";

export default function Avatar({
  id,
  name,
  large = false,
}: {
  id: string;
  name: string;
  large?: boolean;
}) {
  const colours = avatarColours(id);
  const style: CSSProperties = {
    background: colours.background,
    color: colours.foreground,
  };
  return (
    <span
      className={large ? "avatar large" : "avatar"}
      style={style}
      aria-hidden="true"
    >
      {initials(name)}
    </span>
  );
}
