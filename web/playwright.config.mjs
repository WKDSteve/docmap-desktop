// The single-file page of the web variant, opened from disk. No server: `pnpm build` makes the file first.
// The Chrome of the machine is used (`channel`), so no browser is downloaded.

import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  testMatch: /.*\.spec\.mjs/,
  workers: 1,
  fullyParallel: false,
  timeout: 60_000,
  reporter: 'list',
  outputDir: './test-results',
  use: { trace: 'retain-on-failure', screenshot: 'off', video: 'off' },
  projects: [{ name: 'chrome', use: { channel: 'chrome' } }],
});
