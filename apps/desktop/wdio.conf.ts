import path from "node:path";

const application = path.resolve(process.cwd(), "../../target/debug/fishmuse-desktop.exe");

export const config = {
  runner: "local",
  specs: ["./e2e/**/*.e2e.ts"],
  maxInstances: 1,
  services: [
    ["@wdio/tauri-service", {
      appBinaryPath: application,
      driverProvider: "embedded",
      embeddedPort: 4445,
      captureBackendLogs: true,
      captureFrontendLogs: true,
    }],
  ],
  capabilities: [{
    browserName: "tauri",
    "tauri:options": { application },
  }],
  logLevel: "error",
  waitforTimeout: 10_000,
  connectionRetryTimeout: 90_000,
  connectionRetryCount: 2,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    timeout: 60_000,
  },
};
