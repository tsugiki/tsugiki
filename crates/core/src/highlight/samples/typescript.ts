// The rows the view asks for at a time.
export const CHUNK = 512;

export interface Chunk<T> {
  readonly start: number;
  rows: T[];
}

export function chunk<T>(rows: readonly T[], start: number): Chunk<T> | null {
  if (start >= rows.length) return null;
  const end = Math.min(start + CHUNK, rows.length);
  return { start, rows: rows.slice(start, end) as T[] };
}

enum Side { Base, Head }
