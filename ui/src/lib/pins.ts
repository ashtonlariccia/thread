import type { Pin } from "./types";

/** Same button? Kind and target only — a renamed pin is still the same pin. */
export function samePin(a: Pin, b: Pin): boolean {
  return a.kind === b.kind && a.target === b.target;
}

/**
 * Which gap a drag is hovering, given the horizontal midpoint of every chip.
 *
 * "Gap n" means *before* the chip currently at n, and a gap equal to the number
 * of chips means past the right-hand end. Crossing a chip's midpoint — rather
 * than touching its edge — is what commits to swapping with it, so a chip does
 * not flicker between two slots while the pointer sits over its boundary.
 */
export function dropGap(midpoints: number[], x: number): number {
  let gap = 0;
  while (gap < midpoints.length && x > midpoints[gap]) gap++;
  return gap;
}

/**
 * The index a chip dragged from `from` should end up at, in the list as it will
 * be once the chip has been lifted out of it.
 *
 * The subtraction is the whole subtlety: a gap to the *right* of where the chip
 * started counts one position too many, because that count still includes the
 * chip itself.
 */
export function dropIndex(midpoints: number[], x: number, from: number): number {
  const gap = dropGap(midpoints, x);
  return gap > from ? gap - 1 : gap;
}
