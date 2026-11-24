// Lets a page put Apply / Undo (or Reset) in the header's top-right corner.
// The page that owns the changes registers its handlers; the header shows them.

export interface PageActions {
  /** There are changes on screen that haven't been sent to the laptop yet. */
  pending: boolean;
  apply: (() => void) | null;
  undo: (() => void) | null;
  /** Shown instead of Apply / Undo when nothing is pending. */
  reset: (() => void) | null;
}

export const pageActions: PageActions = $state({ pending: false, apply: null, undo: null, reset: null });

export function clearPageActions(): void {
  pageActions.pending = false;
  pageActions.apply = null;
  pageActions.undo = null;
  pageActions.reset = null;
}
