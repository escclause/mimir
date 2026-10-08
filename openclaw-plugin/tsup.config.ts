import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["index.ts"],
  format: ["esm"],
  target: "node20",
  outDir: "dist",
  sourcemap: true,
  clean: true,
  external: ["openclaw", "node:child_process", "node:fs", "node:path", "node:os"],
});
