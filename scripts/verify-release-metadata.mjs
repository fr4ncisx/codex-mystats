import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const metadataResult = spawnSync(
  "cargo",
  ["metadata", "--locked", "--no-deps", "--format-version", "1"],
  { encoding: "utf8" },
);

if (metadataResult.error) {
  throw new Error(`Could not run cargo metadata: ${metadataResult.error.message}`);
}

if (metadataResult.status !== 0) {
  process.stderr.write(metadataResult.stderr);
  throw new Error(`cargo metadata exited with status ${metadataResult.status}.`);
}

const metadata = JSON.parse(metadataResult.stdout);
const packageVersion = metadata.packages.find(
  (packageInfo) => packageInfo.name === "wimu",
)?.version;

if (!packageVersion) {
  throw new Error("Cargo metadata does not contain the wimu package.");
}

const tauriConfig = JSON.parse(readFileSync("tauri.conf.json", "utf8"));
const tauriVersion = tauriConfig.version;

if (packageVersion !== tauriVersion) {
  throw new Error(
    `Cargo.toml version ${packageVersion} does not match tauri.conf.json version ${tauriVersion}.`,
  );
}

const releaseNotesPath = resolve(
  ".github",
  "release-notes",
  `v${tauriVersion}.md`,
);
const releaseNotes = readFileSync(releaseNotesPath, "utf8");
const firstHeading = releaseNotes.split(/\r?\n/, 1)[0];
const noteVersion = firstHeading.match(/^# .+ (v\d+\.\d+\.\d+)$/)?.[1];

if (noteVersion !== `v${tauriVersion}`) {
  throw new Error(
    `Expected release notes for v${tauriVersion}, got "${firstHeading}".`,
  );
}

console.log(`Validated package version ${packageVersion} and ${releaseNotesPath}.`);
