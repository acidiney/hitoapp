import type { ComponentProps } from "react";
import { useLongPress } from "../hooks/useLongPress";

export default function LongPressRow({
  onLongPress,
  ...row
}: ComponentProps<"tr"> & { onLongPress?: () => void }) {
  const press = useLongPress(onLongPress ?? (() => {}));
  return <tr {...row} {...(onLongPress ? press : {})} />;
}
