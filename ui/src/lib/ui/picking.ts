// How the diff and the Definition peek's code pick a word with a click or a double-click (#437, #447): a
// click puts the caret on the word under it, and a double-click selects the word its first click picked.

import { tick } from "svelte";

/** What a region's picks go to: its pick, and how a click changes it. */
export interface Picking<P> {
  /** Picks the word at `caret`, in UTF-16 code units, of the text of `line`, and returns the pick, or drops
   * the pick with no word there. */
  pick(line: HTMLElement, caret: number): P | undefined;
  /** Drops the pick, as for a click on no line of code. */
  drop(): void;
  /** The pick now. A double-click selects only the word its own first click picked. */
  current(): P | undefined;
  /** Picks `pick`'s word again, selected, as for a double-click. */
  select(pick: P): void;
}

/** The click and double-click handlers of a region whose lines of code `lines` names, which take picks for
 * `picking`. A click on a target `keeps` names leaves the pick be, and so does a drag, which selects text
 * to copy. */
export function wordPicker<P>(lines: string, picking: Picking<P>, keeps?: string) {
  /** The pick the last single click made, and the line it is on, which a double-click selects. */
  let clicked: { pick: P; line: HTMLElement } | undefined;
  return {
    /** Picks the word under the caret the click `e` left, and returns the pick. It drops the pick when the
     * caret is on no line of code, or the click landed outside that line: a click past the end of a line, or
     * on a line's number, can leave the caret in some line's code. */
    click(e: MouseEvent): P | undefined {
      // A double-click's second click leaves the word to the double-click.
      if (e.detail > 1) return undefined;
      clicked = undefined;
      const target = e.target;
      if (keeps !== undefined && target instanceof Element && target.closest(keeps)) return undefined;
      const selection = getSelection();
      if (selection === null || !selection.isCollapsed) return undefined;
      const node = selection.focusNode;
      const line = (node instanceof Element ? node : node?.parentElement)?.closest<HTMLElement>(lines);
      if (!node || !line || !(target instanceof Node) || !line.contains(target)) {
        picking.drop();
        return undefined;
      }
      const pick = picking.pick(line, caretIn(line, node, selection.focusOffset));
      if (pick !== undefined) clicked = { pick, line };
      return pick;
    },
    /** Selects the word the double-click's first click picked, if it picked one that is still the pick. The
     * browser's own word can run past an identifier, as through the dot of `self.name`. */
    async doubleClick() {
      const first = clicked;
      clicked = undefined;
      if (first === undefined || picking.current() !== first.pick) return;
      picking.select(first.pick);
      await tick();
      selectPicked(first.line);
    },
  };
}

/** The caret's place in `element`'s text, in UTF-16 code units: the text before offset `offset` into
 * `node`, which is inside it. */
function caretIn(element: Element, node: Node, offset: number): number {
  const before = document.createRange();
  before.setStart(element, 0);
  before.setEnd(node, offset);
  return before.toString().length;
}

/** Selects the word picked on `line`, once it is drawn picked. Only that line's pieces count, so a word
 * picked elsewhere, as in the peek inside the diff, stays out of the selection. */
function selectPicked(line: HTMLElement) {
  const picked = [...line.querySelectorAll(".picked")];
  // From before the word's first piece to after its last: the end moves forward over the pieces, and then
  // the start moves back over them.
  const range = document.createRange();
  for (const piece of picked) range.setEndAfter(piece);
  for (const piece of picked.reverse()) range.setStartBefore(piece);
  getSelection()?.removeAllRanges();
  getSelection()?.addRange(range);
}
