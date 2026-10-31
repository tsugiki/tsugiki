// PROTOTYPE — throwaway. The visible item list: row ids, with collapsed Generated files
// replaced by one placeholder item encoded as -(fileIdx + 1).
import type { FileMeta } from "./types";
import { COLLAPSED_H, COMMENT_EST_H, FILE_H, ROW_H } from "./types";

export function buildView(files: FileMeta[], collapsed: Set<number>): Int32Array {
  let n = 0;
  files.forEach((f, i) => (n += collapsed.has(i) ? 2 : f.end - f.start));
  const view = new Int32Array(n);
  let j = 0;
  files.forEach((f, i) => {
    view[j++] = f.start;
    if (collapsed.has(i)) view[j++] = -(i + 1);
    else for (let r = f.start + 1; r < f.end; r++) view[j++] = r;
  });
  return view;
}

export function estimateHeight(item: number, fileStarts: Set<number>, comments: Set<number>): number {
  if (item < 0) return COLLAPSED_H;
  if (fileStarts.has(item)) return FILE_H;
  if (comments.has(item)) return COMMENT_EST_H;
  return ROW_H;
}
