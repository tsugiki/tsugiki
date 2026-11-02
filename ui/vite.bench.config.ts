// The benchmarks' config: the app's, plus CodSpeed, with Svelte compiled as the app ships rather than
// in dev mode. It's kept apart so that `vite dev` and `vite build` don't load CodSpeed's tooling.
import codspeed from "@codspeed/vitest-plugin";
import { mergeConfig } from "vitest/config";
import base from "./vite.config.ts";

process.env["NODE_ENV"] = "production";

export default mergeConfig(base, { plugins: [codspeed()] });
