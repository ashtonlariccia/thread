/**
 * A tab being dragged, and where it would land.
 *
 * Shared because a drag starts on one pane's strip and can end on another's:
 * the strip the tab left is the one hearing the pointer, and the strip it is
 * over is the one that has to draw the line.
 */

export type DropTarget = {
  pane: number;
  /** The gap in that pane's strip, or null for its body: the end of the strip. */
  gap: number | null;
};

export const tabDrag = $state<{
  /** Where the tab being carried started; null while nothing is. */
  from: { pane: number; index: number } | null;
  over: DropTarget | null;
}>({ from: null, over: null });
