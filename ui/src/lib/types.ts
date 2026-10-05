/**
 * One row in the sidebar.
 *
 * Deliberately bare: the sidebar only needs to list, select and close things.
 * Whatever ends up living there (open files, a tree) extends this.
 */
export type SidebarItem = {
  key: number;
  title: string;
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
