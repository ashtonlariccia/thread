/** One row of the file tree, as the sidebar draws it. */
export type SidebarItem = {
  /**
   * Unique among the rows. Not the path: a folder open as a root can also be
   * on screen inside another root.
   */
  key: string;
  path: string;
  title: string;
  /** Icon URL. */
  icon: string;
  /** How many folders deep the row is; a root is 0. */
  depth: number;
  /** Set on folders, saying whether theirs is unfolded. */
  folder?: "open" | "closed";
  /** One of the folders that was opened, rather than something inside one. */
  root: boolean;
};

/**
 * One button on the pinned strip.
 *
 * `kind` says what sort of thing it is and `target` identifies it. Nothing
 * produces pins yet, so `kind` is an open string — each feature that learns to
 * pin claims one.
 */
export type Pin = {
  kind: string;
  target: string;
  label: string;
};
