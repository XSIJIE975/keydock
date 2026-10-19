import { existsSync, readFileSync, writeFileSync } from "node:fs";

const packageJson = JSON.parse(readFileSync("package.json", "utf8"));
const version = packageJson.version;

const tauriConfigPath = "src-tauri/tauri.conf.json";
const tauriConfigText = readFileSync(tauriConfigPath, "utf8");
writeFileSync(
  tauriConfigPath,
  tauriConfigText.replace(/("version"\s*:\s*")[^"]+(")/, `$1${version}$2`),
);

// The workspace crates inherit this version.
const workspaceCargoPath = "Cargo.toml";
const workspaceCargo = readFileSync(workspaceCargoPath, "utf8");
const nextWorkspaceCargo = workspaceCargo.replace(
  /(\[workspace\.package\][\s\S]*?version\s*=\s*")[^"]+(")/,
  `$1${version}$2`,
);
writeFileSync(workspaceCargoPath, nextWorkspaceCargo);

const cargoLockPath = "Cargo.lock";
if (existsSync(cargoLockPath)) {
  let lock = readFileSync(cargoLockPath, "utf8");
  for (const name of ["keydock", "keydock-core", "keydock-browser"]) {
    lock = lock.replace(
      new RegExp(`(name = "${name}"\\nversion = ")[^"]+(")`),
      `$1${version}$2`,
    );
  }
  writeFileSync(cargoLockPath, lock);
}

const changelogPath = "CHANGELOG.md";
if (existsSync(changelogPath)) {
  const changelog = readFileSync(changelogPath, "utf8");
  writeFileSync(
    changelogPath,
    changelog
      .replace(/^### Major Changes/gm, "### 破坏性变更")
      .replace(/^### Minor Changes/gm, "### 新功能")
      .replace(/^### Patch Changes/gm, "### 修复"),
  );
}

console.log(`Synced version ${version} to Tauri configuration and workspace`);
