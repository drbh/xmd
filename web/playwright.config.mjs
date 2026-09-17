import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  timeout: 60_000,
  workers: 1,
  use: { channel: "chrome", baseURL: "http://127.0.0.1:4173", viewport: { width: 1280, height: 900 } },
  webServer: { command: "node serve.mjs", url: "http://127.0.0.1:4173", reuseExistingServer: !process.env.CI },
});
