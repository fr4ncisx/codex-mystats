import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const playwrightCli = path.join(projectRoot, "node_modules", "playwright", "cli.js");
const browserCache = path.join(projectRoot, ".playwright-browsers");

const result = spawnSync(process.execPath, [playwrightCli, ...process.argv.slice(2)], {
  cwd: projectRoot,
  env: { ...process.env, PLAYWRIGHT_BROWSERS_PATH: browserCache },
  stdio: "inherit",
});

if (result.error) {
  process.stderr.write(`Could not start Playwright: ${result.error.message}\n`);
  process.exitCode = 1;
} else {
  process.exitCode = result.status ?? 1;
}
