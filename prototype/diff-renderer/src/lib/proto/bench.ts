// PROTOTYPE — throwaway. Repeatable scroll benchmarks, so Windows and macOS numbers compare.
const frame = () => new Promise<number>((r) => requestAnimationFrame(r));

/** True when unrendered content is inside the viewport. Variants may override via window.__protoBlank. */
function hasBlank(el: HTMLElement): boolean {
  const custom = (window as any).__protoBlank as (() => boolean) | undefined;
  if (custom) return custom();
  const vr = el.getBoundingClientRect();
  for (const b of el.querySelectorAll(".blank")) {
    const r = b.getBoundingClientRect();
    if (r.bottom > vr.top && r.top < vr.bottom) return true;
  }
  return false;
}

export interface ScrollResult {
  frames: number;
  avgFps: number;
  p95: number;
  worst: number;
  over25: number;
  blankFrames: number;
  coveredPx: number;
}

export async function autoscroll(el: HTMLElement, pxPerSec: number, maxMs = 15000): Promise<ScrollResult> {
  el.scrollTop = 0;
  await frame();
  await frame();
  const dts: number[] = [];
  let blankFrames = 0;
  const t0 = performance.now();
  let last = t0;
  let pos = 0;
  const max = el.scrollHeight - el.clientHeight;
  while (pos < max && last - t0 < maxMs) {
    const now = await frame();
    const dt = now - last;
    last = now;
    dts.push(dt);
    if (hasBlank(el)) blankFrames++;
    pos = Math.min(max, pos + (pxPerSec * dt) / 1000);
    el.scrollTop = pos;
  }
  const sorted = [...dts].sort((a, b) => a - b);
  const avg = dts.reduce((a, b) => a + b, 0) / dts.length;
  return {
    frames: dts.length,
    avgFps: Math.round(1000 / avg),
    p95: +sorted[Math.floor(sorted.length * 0.95)].toFixed(1),
    worst: +sorted[sorted.length - 1].toFixed(1),
    over25: dts.filter((d) => d > 25).length,
    blankFrames,
    coveredPx: Math.round(pos),
  };
}

/** Random jumps (like "go to file" or "next comment"); time until the viewport is fully rendered. */
export async function jumps(el: HTMLElement, n = 30): Promise<{ avg: number; worst: number }> {
  const times: number[] = [];
  let seed = 12345;
  const rand = () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
  for (let i = 0; i < n; i++) {
    const t0 = performance.now();
    el.scrollTop = rand() * (el.scrollHeight - el.clientHeight);
    await frame();
    await frame();
    while (hasBlank(el) && performance.now() - t0 < 5000) await frame();
    times.push(performance.now() - t0);
  }
  return { avg: +(times.reduce((a, b) => a + b, 0) / n).toFixed(1), worst: +Math.max(...times).toFixed(1) };
}
