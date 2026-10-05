/** Tree rows are keyed by their path. */
export type SidebarKey = string;

/** One row in the sidebar. */
export type SidebarItem = {
  key: SidebarKey;
  title: string;
  /** Second line of the hover card: for a file, its full path. */
  detail?: string;
  /** Icon URL. A row without one gets a plain dot. */
  icon?: string;
  /** In a tree: how many folders deep the row is. */
  depth?: number;
  /** In a tree: set on folders, saying whether theirs is unfolded. */
  folder?: "open" | "closed";
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
