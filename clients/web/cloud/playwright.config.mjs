import { defineConfig } from "@playwright/test";
// Browser tests for the hosted app: the Worker, a local D1, and the Room
// Durable Object, all under wrangler dev with development identities.
export default defineConfig({
  testDir: "./test",
  testMatch: "**/*.spec.mjs",
  timeout: 90_000,
  workers: 1,
  use: { channel: "chrome", baseURL: "http://127.0.0.1:8791", viewport: { width: 1200, height: 800 } },
  webServer: {
    command: "node build.mjs --site-only && rm -rf .wrangler/test-state && npx wrangler d1 migrations apply xmd-docs --local --persist-to .wrangler/test-state && npx wrangler dev --var DEV_AUTH:1 --port 8791 --persist-to .wrangler/test-state",
    url: "http://127.0.0.1:8791/docs/",
    timeout: 180_000,
    reuseExistingServer: !process.env.CI,
  },
});
