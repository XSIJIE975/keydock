import { execFileSync } from "node:child_process";

function getRepoFromOptions(options) {
  return options?.repo ?? "XSIJIE975/keydock";
}

function getCommitSubject(commit) {
  if (!commit) return "";
  try {
    return execFileSync("git", ["log", "-1", "--format=%s", commit], {
      encoding: "utf8",
    }).trim();
  } catch {
    return "";
  }
}

function getChangeLink(changeset, options) {
  if (!changeset.commit) return "";
  const repo = getRepoFromOptions(options);
  const subject = getCommitSubject(changeset.commit);
  const prMatch = subject.match(/\(#(\d+)\)/);
  if (prMatch) {
    return `[#${prMatch[1]}](https://github.com/${repo}/pull/${prMatch[1]})`;
  }
  return `[${changeset.commit.slice(0, 7)}](https://github.com/${repo}/commit/${changeset.commit})`;
}

async function getReleaseLine(changeset, _type, options) {
  const [firstLine, ...futureLines] = changeset.summary
    .split(/\r?\n/)
    .map((line) => line.trimEnd());
  const link = getChangeLink(changeset, options);
  let line = `- ${firstLine}`;
  if (link) line += ` ${link}`;
  if (futureLines.length > 0) {
    line += `\n${futureLines.map((item) => `  ${item}`).join("\n")}`;
  }
  return line;
}

async function getDependencyReleaseLine(_changesets, dependenciesUpdated) {
  if (dependenciesUpdated.length === 0) return "";
  return dependenciesUpdated
    .map((dependency) => {
      const name = dependency.name || dependency.packageJson?.name;
      return `- 更新依赖 \`${name}\` 至 \`${dependency.newVersion}\``;
    })
    .join("\n");
}

export default {
  getReleaseLine,
  getDependencyReleaseLine,
};
