import { existsSync, readFileSync, writeFileSync } from "node:fs";

const markdown = readFileSync("CHANGELOG.md", "utf8");
const outputPath = "src-tauri/changelog.json";

let previousDates = {};
if (existsSync(outputPath)) {
  try {
    const previous = JSON.parse(readFileSync(outputPath, "utf8"));
    previousDates = Object.fromEntries(
      previous.map((entry) => [entry.version, entry.date]),
    );
  } catch {
    previousDates = {};
  }
}

const today = new Date().toISOString().slice(0, 10);
const sections = [];
const headingPattern = /^##\s+(.+)$/gm;
const matches = [...markdown.matchAll(headingPattern)];

for (let index = 0; index < matches.length; index += 1) {
  const version = matches[index][1].trim().replace(/^v/, "");
  const start = matches[index].index + matches[index][0].length;
  const end = index + 1 < matches.length ? matches[index + 1].index : markdown.length;
  const body = markdown.slice(start, end);
  const notes = body
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.startsWith("- "))
    .map((line) => line.slice(2).trim())
    .filter(Boolean);
  if (!version || notes.length === 0) continue;
  sections.push({
    version,
    date: previousDates[version] ?? today,
    notes,
  });
}

writeFileSync(outputPath, `${JSON.stringify(sections, null, 2)}\n`);
console.log(`Wrote ${outputPath} with ${sections.length} version(s)`);
