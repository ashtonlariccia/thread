/** One row in a sidebar. */
export type SidebarItem = {
  key: number;
  title: string;
  /** Second line of the hover card: for a file, its full path. */
  detail?: string;
  /** Icon URL. A row without one gets a plain dot. */
  icon?: string;
  /** Has unsaved changes. */
  dirty?: boolean;
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
