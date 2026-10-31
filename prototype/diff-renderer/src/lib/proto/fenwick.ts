// PROTOTYPE — throwaway. Prefix sums of row heights with O(log n) update and offset lookup.
export class Fenwick {
  readonly n: number;
  private t: Float64Array;

  constructor(heights: Float64Array) {
    this.n = heights.length;
    this.t = new Float64Array(this.n + 1);
    for (let i = 1; i <= this.n; i++) {
      this.t[i] += heights[i - 1];
      const j = i + (i & -i);
      if (j <= this.n) this.t[j] += this.t[i];
    }
  }

  add(i: number, delta: number) {
    for (i++; i <= this.n; i += i & -i) this.t[i] += delta;
  }

  /** Sum of heights[0..i). */
  prefix(i: number): number {
    let s = 0;
    for (; i > 0; i -= i & -i) s += this.t[i];
    return s;
  }

  total(): number {
    return this.prefix(this.n);
  }

  /** Index of the item containing `offset`. */
  indexAt(offset: number): number {
    let pos = 0;
    let step = 1;
    while (step * 2 <= this.n) step *= 2;
    for (; step > 0; step >>= 1) {
      if (pos + step <= this.n && this.t[pos + step] <= offset) {
        pos += step;
        offset -= this.t[pos];
      }
    }
    return Math.min(pos, this.n - 1);
  }
}
