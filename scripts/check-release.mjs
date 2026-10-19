import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

function parseVersion(value) {
  const [major = 0, minor = 0, patch = 0] = value
    .replace(/^v/, "")
    .split(".")
    .map((part) => Number(part));
  return [major, minor, patch];
}

function compareVersions(left, right) {
  const leftParts = parseVersion(left);
  const rightParts = parseVersion(right);
  for (let index = 0; index < leftParts.length; index += 1) {
    if (leftParts[index] > rightParts[index]) return 1;
    if (leftParts[index] < rightParts[index]) return -1;
  }
  return 0;
}

function git(args) {
  try {
    return execFileSync("git", args, { encoding: "utf8" }).trim();
  } catch {
    return "";
  }
}

const packageJson = JSON.parse(readFileSync("package.json", "utf8"));
const overrideVersion = (process.env.RELEASE_VERSION ?? "").trim();
const forceBuild = overrideVersion !== "";
const version = overrideVersion || packageJson.version;

let previousVersion = null;
const previousPackage = git(["show", "HEAD^1:package.json"]);
if (previousPackage) {
  try {
    previousVersion = JSON.parse(previousPackage).version;
  } catch {
    previousVersion = null;
  }
}

const tags = git(["tag", "--list", "v*"])
  .split(/\r?\n/)
  .filter(Boolean)
  .sort((left, right) => compareVersions(right, left));
const latestTag = tags[0]?.replace(/^v/, "") ?? "";

const changedFromPreviousCommit =
  previousVersion !== null && compareVersions(version, previousVersion) > 0;
const changedFromLatestTag =
  latestTag !== "" && compareVersions(version, latestTag) > 0;
const tagAlreadyExists = tags.includes(`v${version}`);
const shouldBuild =
  forceBuild || (!tagAlreadyExists && (changedFromPreviousCommit || changedFromLatestTag));

process.stdout.write(
  [
    `should_build=${shouldBuild ? "true" : "false"}`,
    `version=${version}`,
    `latest_tag=${latestTag}`,
    `previous_version=${previousVersion ?? ""}`,
  ].join("\n") + "\n",
);
