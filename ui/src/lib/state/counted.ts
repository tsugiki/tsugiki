// A count with its noun, as the window words it everywhere.

/** `count` with the noun `one` or `many` after it, in thousands with commas: "1 file", "1,204 files". */
export const counted = (count: number, one: string, many: string) =>
  `${count.toLocaleString("en-US")} ${count === 1 ? one : many}`;
