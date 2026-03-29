/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import tailwindcss from "@tailwindcss/vite";

const isTest = process.env.NODE_ENV === "test" || process.env.VITEST;

export default defineConfig({
  plugins: [solid({ dev: !isTest, hot: !isTest }), tailwindcss()],
  server: {
    proxy: {
      "/api": {
        target: "http://localhost:3001",
        changeOrigin: true,
      },
    },
  },
  test: {
    environment: "happy-dom",
    globals: true,
    isolate: false,
    pool: "vmThreads",
    deps: {
      optimizer: {
        web: {
          enabled: true,
        },
      },
    },
  },
});
