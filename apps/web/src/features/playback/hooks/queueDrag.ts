export const SLIDE_MS = 240;
export const SLIDE_EASING = "cubic-bezier(.2,0,0,1)";
const EDGE_ZONE = 56;
const EDGE_MAX_SPEED = 18;

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}

export function clampTravel(
  travel: number,
  from: number,
  first: number,
  last: number,
  row: number,
): number {
  return clamp(travel, (first - from) * row, (last - 1 - from) * row);
}

export function dropSlot(
  from: number,
  travel: number,
  first: number,
  last: number,
  row: number,
): number {
  return clamp(Math.round(from + travel / row), first, last - 1);
}

export function shiftFor(
  index: number,
  from: number,
  over: number,
  row: number,
): number {
  if (index === from) return 0;
  if (from < over && index > from && index <= over) return -row;
  if (over < from && index >= over && index < from) return row;
  return 0;
}

export function insertionIndex(from: number, over: number): number {
  return over > from ? over + 1 : over;
}

export function edgeSpeed(pointerY: number, top: number, bottom: number) {
  if (pointerY < top + EDGE_ZONE)
    return -Math.ceil(
      EDGE_MAX_SPEED * clamp((top + EDGE_ZONE - pointerY) / EDGE_ZONE, 0, 1),
    );
  if (pointerY > bottom - EDGE_ZONE)
    return Math.ceil(
      EDGE_MAX_SPEED *
        clamp((pointerY - (bottom - EDGE_ZONE)) / EDGE_ZONE, 0, 1),
    );
  return 0;
}
