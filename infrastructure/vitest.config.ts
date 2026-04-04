import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    globals: true,
    isolate: false,
    pool: "vmThreads",
    passWithNoTests: true,
    alias: {
      "@pulumi/neon": new URL("src/__mocks__/neon.ts", import.meta.url)
        .pathname,
    },
  },
});
