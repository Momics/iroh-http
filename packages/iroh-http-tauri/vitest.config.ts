import { defineConfig } from "vitest/config";
import { resolve } from "path";

export default defineConfig({
  test: {
    environment: "jsdom",
    root: resolve(__dirname),
    include: ["guest-js/__tests__/**/*.test.ts"],
  },
});
