/** The places the user has been, in order, which `Alt+←` and `Alt+→`, the arrows over the Detail pane and
 * the mouse's back and forward buttons go through (#186, #586). */
export class Visits<T> {
  readonly #same: (one: T, other: T) => boolean;
  #places = $state<T[]>([]);
  /** Where among the places the user is, or -1 before the first visit. */
  #at = $state(-1);

  /** Visits that `same` says name one place count once, when they come one after the other. */
  constructor(same: (one: T, other: T) => boolean) {
    this.#same = same;
  }

  /** Goes to `place`, which drops the places ahead of where the user was. */
  visit(place: T) {
    const here = this.#places[this.#at];
    if (here !== undefined && this.#same(here, place)) return;
    this.#places = [...this.#places.slice(0, this.#at + 1), place];
    this.#at += 1;
  }

  /** The place before, if there is one, which the user is now at. */
  back(): T | undefined {
    return this.go(-1);
  }

  /** The place ahead, if there is one, which the user is now at. */
  forward(): T | undefined {
    return this.go(1);
  }

  /** The place `steps` away, back for a negative number, if there is one, which the user is now at. */
  go(steps: number): T | undefined {
    const place = this.#places[this.#at + steps];
    if (place !== undefined) this.#at += steps;
    return place;
  }

  /** Up to `limit` places before this one, the nearest first. */
  behind(limit: number): T[] {
    return this.#places.slice(Math.max(0, this.#at - limit), Math.max(0, this.#at)).reverse();
  }

  /** Up to `limit` places ahead of this one, the nearest first. */
  ahead(limit: number): T[] {
    return this.#places.slice(this.#at + 1, this.#at + 1 + limit);
  }
}
