import { readFileSync } from "node:fs";

const repo = process.env.GITHUB_REPOSITORY ?? "XSIJIE975/keydock";
const token = process.env.GITHUB_TOKEN;
const releaseId = process.env.RELEASE_ID;

if (!token || !releaseId) {
  throw new Error("GITHUB_TOKEN and RELEASE_ID are required");
}

// Mapping must match the release workflow matrix. Tauri produces an
// `.app.tar.gz` updater artifact on macOS; some bundler versions also emit a
// `.dmg.tar.gz`. Both point at the same updater payload.
const PLATFORM_MAP = [
  {
    pattern: /_aarch64\.app\.tar\.gz\.sig$/,
    keys: ["darwin-aarch64", "darwin-aarch64-app"],
  },
  {
    pattern: /_aarch64\.dmg\.tar\.gz\.sig$/,
    keys: ["darwin-aarch64", "darwin-aarch64-app"],
  },
  {
    pattern: /_x64-setup\.exe\.sig$/,
    keys: ["windows-x86_64", "windows-x86_64-nsis"],
  },
];

function headers(extra = {}) {
  return {
    Authorization: `Bearer ${token}`,
    "X-GitHub-Api-Version": "2022-11-28",
    ...extra,
  };
}

const apiRoot = `https://api.github.com/repos/${repo}`;
const releaseResponse = await fetch(`${apiRoot}/releases/${releaseId}`, {
  headers: headers(),
});
if (!releaseResponse.ok) {
  throw new Error(`获取 Release 失败：${releaseResponse.status}`);
}
const release = await releaseResponse.json();
const assets = release.assets;

const signatureAssets = assets.filter((asset) => asset.name.endsWith(".sig"));
const platforms = {};
for (const asset of signatureAssets) {
  const mapping = PLATFORM_MAP.find((entry) => entry.pattern.test(asset.name));
  if (!mapping) {
    console.log(`跳过未映射的签名文件：${asset.name}`);
    continue;
  }
  const binaryName = asset.name.replace(/\.sig$/, "");
  const binaryAsset = assets.find((item) => item.name === binaryName);
  if (!binaryAsset) {
    throw new Error(`找不到签名对应的产物：${binaryName}`);
  }
  const sigResponse = await fetch(
    `${apiRoot}/releases/assets/${asset.id}`,
    { headers: headers({ Accept: "application/octet-stream" }) },
  );
  if (!sigResponse.ok) {
    throw new Error(`下载 ${asset.name} 失败：${sigResponse.status}`);
  }
  const signature = (await sigResponse.text()).trim();
  for (const key of mapping.keys) {
    platforms[key] = {
      signature,
      url: `${apiRoot}/releases/assets/${binaryAsset.id}`,
    };
  }
}

const version = release.tag_name.replace(/^v/, "");
const notes = readFileSync("release-body.md", "utf8")
  .replace(/^##\s+[^\n]*\n+/, "")
  .trim();

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms,
};

const latestAsset = assets.find((asset) => asset.name === "latest.json");
if (latestAsset) {
  const deleteResponse = await fetch(
    `${apiRoot}/releases/assets/${latestAsset.id}`,
    { method: "DELETE", headers: headers() },
  );
  if (!deleteResponse.ok) {
    throw new Error(`删除旧 latest.json 失败：${deleteResponse.status}`);
  }
}

const uploadUrl = release.upload_url.split("{")[0];
const payload = JSON.stringify(manifest);
const uploadResponse = await fetch(`${uploadUrl}?name=latest.json`, {
  method: "POST",
  headers: headers({
    "Content-Type": "application/octet-stream",
    "Content-Length": Buffer.byteLength(payload),
  }),
  body: payload,
});
if (!uploadResponse.ok) {
  throw new Error(`上传 latest.json 失败：${uploadResponse.status}`);
}

console.log(
  `已重建 v${version} 更新清单：${Object.keys(platforms).length} 个平台条目，更新说明 ${notes.length} 字`,
);
