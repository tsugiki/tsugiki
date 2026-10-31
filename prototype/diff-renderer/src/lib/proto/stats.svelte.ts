// PROTOTYPE — throwaway. Everything the HUD shows lives here.
export const stats = $state({
  fps: 0,
  p95: 0,
  worst: 0,
  longFrames: 0,
  dom: 0,
  heapMB: null as number | null,
  load: {} as Record<string, number>,
  payloadMB: 0,
  ipc: { count: 0, avg: 0, p95: 0 },
  results: [] as string[],
  busy: "",
});

const ipcTimes: number[] = [];

export function recordIpc(ms: number) {
  ipcTimes.push(ms);
  const sorted = [...ipcTimes].sort((a, b) => a - b);
  stats.ipc = {
    count: ipcTimes.length,
    avg: ipcTimes.reduce((a, b) => a + b, 0) / ipcTimes.length,
    p95: sorted[Math.floor(sorted.length * 0.95)] ?? 0,
  };
}

export function resetIpc() {
  ipcTimes.length = 0;
  stats.ipc = { count: 0, avg: 0, p95: 0 };
}

export function startFrameMonitor(): () => void {
  const dts: number[] = [];
  let last = performance.now();
  let raf = 0;
  const tick = (now: number) => {
    dts.push(now - last);
    last = now;
    if (dts.length > 120) dts.shift();
    raf = requestAnimationFrame(tick);
  };
  raf = requestAnimationFrame(tick);
  const timer = setInterval(() => {
    if (!dts.length) return;
    const sorted = [...dts].sort((a, b) => a - b);
    const avg = dts.reduce((a, b) => a + b, 0) / dts.length;
    stats.fps = Math.round(1000 / avg);
    stats.p95 = sorted[Math.floor(sorted.length * 0.95)];
    stats.worst = sorted[sorted.length - 1];
    stats.longFrames = dts.filter((d) => d > 50).length;
    stats.dom = document.getElementsByTagName("*").length;
    const mem = (performance as any).memory;
    stats.heapMB = mem ? mem.usedJSHeapSize / 1e6 : null;
  }, 500);
  return () => {
    cancelAnimationFrame(raf);
    clearInterval(timer);
  };
}
