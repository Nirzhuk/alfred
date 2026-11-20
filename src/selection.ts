/** Surfaces where native text selection is allowed. */
const SELECTABLE_SELECTOR =
  'input, textarea, select, [contenteditable="true"], .user-select-text, .output-modal-body, .output-modal-summary';

type SelectionTarget = {
  nodeType?: number;
  parentElement?: Element | null;
  closest?: (selector: string) => Element | null;
};

/**
 * `selectstart` targets can be text nodes, which have no `closest`. Resolve a
 * text target to its parent element before asking whether it is selectable.
 */
export function isSelectableTarget(target: EventTarget | null): boolean {
  const node = target as SelectionTarget | null;
  if (!node) return false;
  const element = node.nodeType === 1 ? node : node.parentElement;
  return Boolean(element?.closest?.(SELECTABLE_SELECTOR));
}
