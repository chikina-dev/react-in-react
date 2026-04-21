import { defineConfig } from "@playwright/test";

const localHost = "127.0.0.1";
const localPort = 4173;
const localBaseUrl = `http://${localHost}:${localPort}`;

export default defineConfig({
  testDir: "./e2e",
  testMatch: "**/*.e2e.ts",
  timeout: 60_000,
  use: {
    baseURL: localBaseUrl,
    browserName: "chromium",
    headless: true,
  },
  webServer: {
    command: `vp dev --host ${localHost} --port ${localPort}`,
    url: localBaseUrl,
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
