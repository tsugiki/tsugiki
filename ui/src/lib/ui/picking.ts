// Helpers the diff and the Definition peek's code share to pick a word with a click or a double-click (#437,
// #447): where the caret a click left sits in a line's text, and the selection of the word picked.

/** The caret's place in `element`'s text, in UTF-16 code units: the text before offset `offset` into
 * `node`, which is inside it. */
export function caretIn(element: Element, node: Node, offset: number): number {
  const before = document.createRange();
  before.setStart(element, 0);
  before.setEnd(node, offset);
  return before.toString().length;
}

/** Selects the word picked in `region`, once it is drawn picked. */
export function selectPicked(region: HTMLElement) {
  const picked = [...region.querySelectorAll(".picked")];
  // From before the word's first piece to after its last: the end moves forward over the pieces, and then
  // the start moves back over them.
  const range = document.createRange();
  for (const piece of picked) range.setEndAfter(piece);
  for (const piece of picked.reverse()) range.setStartBefore(piece);
  getSelection()?.removeAllRanges();
  getSelection()?.addRange(range);
}
