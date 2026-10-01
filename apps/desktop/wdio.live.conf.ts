import path from "node:path";

const configuredBinary = process.env.FISHMUSE_LIVE_BINARY;
if (!configuredBinary) {
  throw new Error("FISHMUSE_LIVE_BINARY is required; use scripts/test-live-fishmuse.ps1.");
}
const application = path.resolve(configuredBinary);

export const config = {
  runner: "local",
  specs: ["./e2e/playback-live.e2e.ts"],
  maxInstances: 1,
  services: [
    ["@wdio/tauri-service", {
      appBinaryPath: application,
      driverProvider: "embedded",
      embeddedPort: 4446,
      captureBackendLogs: true,
      captureFrontendLogs: true,
    }],
  ],
  capabilities: [{
    browserName: "tauri",
    "tauri:options": { application },
  }],
  logLevel: "error",
  waitforTimeout: 15_000,
  connectionRetryTimeout: 90_000,
  connectionRetryCount: 2,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    timeout: 120_000,
  },
};
