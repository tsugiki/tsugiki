/** The places the user has been, in order, which `Alt+←` and `Alt+→` go back and forward through (#186). */
export class Visits<T> {
  readonly #same: (one: T, other: T) => boolean;
  readonly #places: T[] = [];
  /** Where among the places the user is, or -1 before the first visit. */
  #at = -1;

  /** Visits that `same` says name one place count once, when they come one after the other. */
  constructor(same: (one: T, other: T) => boolean) {
    this.#same = same;
  }

  /** Goes to `place`, which drops the places ahead of where the user was. */
  visit(place: T) {
    const here = this.#places[this.#at];
    if (here !== undefined && this.#same(here, place)) return;
    this.#at += 1;
    this.#places.splice(this.#at, this.#places.length, place);
  }

  /** The place before, if there is one, which the user is now at. */
  back(): T | undefined {
    return this.#step(-1);
  }

  /** The place ahead, if there is one, which the user is now at. */
  forward(): T | undefined {
    return this.#step(1);
  }

  #step(step: 1 | -1): T | undefined {
    const place = this.#places[this.#at + step];
    if (place !== undefined) this.#at += step;
    return place;
  }
}
