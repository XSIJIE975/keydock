import { readFileSync, writeFileSync } from "node:fs";

const packageJson = JSON.parse(readFileSync("package.json", "utf8"));
const overrideVersion = (process.env.RELEASE_VERSION ?? "").trim();
const version = overrideVersion || packageJson.version;

function extractSection(content, value) {
  const heading = `## ${value}`;
  const start = content.indexOf(heading);
  if (start < 0) return "";
  const rest = content.slice(start + heading.length);
  const nextHeading = rest.search(/^##\s/m);
  return rest.slice(0, nextHeading < 0 ? rest.length : nextHeading).trim();
}

let body = "";
try {
  body = extractSection(readFileSync("CHANGELOG.md", "utf8"), version);
} catch {
  body = "";
}

writeFileSync(
  "release-body.md",
  body ? `## ${version}\n\n${body}\n` : `## ${version}\n\n暂无变更说明。\n`,
);
console.log(`Wrote release-body.md for ${version}`);
