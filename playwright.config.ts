import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/browser",
  use: {
    browserName: "chromium",
    headless: true,
    viewport: { width: 1280, height: 800 },
  },
});
