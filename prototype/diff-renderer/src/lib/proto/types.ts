// PROTOTYPE — throwaway.
export interface FileMeta {
  path: string;
  start: number;
  end: number;
  adds: number;
  dels: number;
  generated: boolean;
  binary: boolean;
  max_cols: number;
}

export interface LoadInfo {
  dataset: string;
  range: string;
  repo: string;
  files: FileMeta[];
  total_rows: number;
  comment_rows: number[];
  css: string;
  eager: boolean;
  git_ms: number;
  attr_ms: number;
  parse_ms: number;
  highlight_ms: number;
}

export interface Row {
  k: number;
  o: number;
  n: number;
  h: string;
}

export const K = { FILE: 0, HUNK: 1, CTX: 2, ADD: 3, DEL: 4, COMMENT: 5, NOTE: 6 } as const;

export const ROW_H = 20;
export const FILE_H = 34;
export const COLLAPSED_H = 30;
export const COMMENT_EST_H = 140;
