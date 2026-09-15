import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const root = path.resolve(__dirname, '..');
const binDir = path.join(root, 'src-tauri', 'binaries');
const dllDir = path.join(binDir, 'dll');

const VERSION = process.env.SCRCPY_VERSION || '4.0';
const FORCE = process.argv.includes('--force') || process.argv.includes('-f');

const KNOWN_SHA256 = {
  '4.0': {
    'scrcpy-win64-v4.0.zip': '75dbeb5b00e6f64292f26f70900ae55ca397786bdfb0b9bbeb481a0549047457',
    'scrcpy-win32-v4.0.zip': '5f860ad2fc66042bd490e31b983d5e40fd749314f28f0dcb9fec697fa89861be',
    'scrcpy-macos-aarch64-v4.0.tar.gz': 'f5167fe047fe4a2ae2c2ea8634c7145a4d64d0b6005f24bb45639a965b8c60d4',
    'scrcpy-macos-x86_64-v4.0.tar.gz': 'b83169f856d7022ed0e4428d98acea18dde2d63f49611b52ea137577ce4efe6b',
    'scrcpy-linux-x86_64-v4.0.tar.gz': '7daf05af5d575862e62b068cf6852d6068faf7ef3178f3735e3953e778fbf0ab',
  },
};

function getHostTargetTriple() {
  try {
    const hostTuple = execSync('rustc --print host-tuple', { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim();
    if (hostTuple) return hostTuple;
  } catch {
    // fallback to rustc -vV if --print host-tuple failed
    try {
      const vV = execSync('rustc -vV', { stdio: ['ignore', 'pipe', 'ignore'] }).toString();
      const match = vV.match(/^host:\s*(.+)$/m);
      if (match && match[1]) return match[1].trim();
    } catch {
      // ignore
    }
  }

  // Derive reasonable defaults when rustc is not installed
  const platform = process.platform;
  const arch = process.arch;
  if (platform === 'win32') {
    return arch === 'arm64' ? 'aarch64-pc-windows-msvc' : 'x86_64-pc-windows-msvc';
  } else if (platform === 'darwin') {
    return arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
  } else if (platform === 'linux') {
    return arch === 'arm64' ? 'aarch64-unknown-linux-gnu' : 'x86_64-unknown-linux-gnu';
  }
  return `${arch}-unknown-${platform}`;
}

function resolveAssetInfo(platform, arch) {
  if (platform === 'win32') {
    const asset = arch === 'ia32' ? `scrcpy-win32-v${VERSION}.zip` : `scrcpy-win64-v${VERSION}.zip`;
    return { asset, ext: 'zip', exeExt: '.exe' };
  } else if (platform === 'darwin') {
    const asset = arch === 'arm64'
      ? `scrcpy-macos-aarch64-v${VERSION}.tar.gz`
      : `scrcpy-macos-x86_64-v${VERSION}.tar.gz`;
    return { asset, ext: 'tar.gz', exeExt: '' };
  } else if (platform === 'linux') {
    const asset = `scrcpy-linux-x86_64-v${VERSION}.tar.gz`;
    return { asset, ext: 'tar.gz', exeExt: '' };
  }
  throw new Error(`Unsupported platform: ${platform} (${arch})`);
}

async function downloadFile(url, destPath) {
  console.log(`Downloading ${url} ...`);
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) {
    throw new Error(`Failed to download ${url}: ${res.status} ${res.statusText}`);
  }
  const arrayBuffer = await res.arrayBuffer();
  fs.writeFileSync(destPath, Buffer.from(arrayBuffer));
}

function computeSha256(filePath) {
  const fileBuffer = fs.readFileSync(filePath);
  return crypto.createHash('sha256').update(fileBuffer).digest('hex');
}

async function fetchSha256Sums(version) {
  try {
    const sumsUrl = `https://github.com/Genymobile/scrcpy/releases/download/v${version}/SHA256SUMS.txt`;
    const res = await fetch(sumsUrl, { redirect: 'follow' });
    if (!res.ok) return {};
    const text = await res.text();
    const map = {};
    for (const line of text.split('\n')) {
      const match = line.trim().match(/^([0-9a-fA-F]{64})\s+\*?(.+)$/);
      if (match) {
        map[match[2].trim()] = match[1].toLowerCase();
      }
    }
    return map;
  } catch {
    return {};
  }
}

async function extractZip(zipPath, targetDir) {
  // Use PowerShell on Windows or unzip if available
  if (process.platform === 'win32') {
    execSync(`powershell -NoProfile -ExecutionPolicy Bypass -Command "Expand-Archive -Path '${zipPath}' -DestinationPath '${targetDir}' -Force"`, { stdio: 'inherit' });
  } else {
    execSync(`unzip -q -o "${zipPath}" -d "${targetDir}"`, { stdio: 'inherit' });
  }
}

async function extractTarGz(tarGzPath, targetDir) {
  fs.mkdirSync(targetDir, { recursive: true });
  execSync(`tar -xzf "${tarGzPath}" -C "${targetDir}"`, { stdio: 'inherit' });
}

async function main() {
  const triple = getHostTargetTriple();
  const { asset, ext, exeExt } = resolveAssetInfo(process.platform, process.arch);

  const scrcpySidecar = path.join(binDir, `scrcpy-${triple}${exeExt}`);
  const adbSidecar = path.join(binDir, `adb-${triple}${exeExt}`);
  const serverDst = path.join(binDir, 'scrcpy-server');

  if (!FORCE && fs.existsSync(scrcpySidecar) && fs.existsSync(adbSidecar) && fs.existsSync(serverDst)) {
    console.log('Binaries already present (use --force to re-download). Skipping.');
    return;
  }

  const url = `https://github.com/Genymobile/scrcpy/releases/download/v${VERSION}/${asset}`;
  const tmpDir = path.join(os.tmpdir(), `anyleap-scrcpy-${Date.now()}`);
  fs.mkdirSync(tmpDir, { recursive: true });
  const archivePath = path.join(tmpDir, asset);

  await downloadFile(url, archivePath);

  // SHA256 integrity check
  let expectedSha256 = KNOWN_SHA256[VERSION]?.[asset];
  if (!expectedSha256) {
    const sumsMap = await fetchSha256Sums(VERSION);
    expectedSha256 = sumsMap[asset];
  }

  const actualSha256 = computeSha256(archivePath);
  if (expectedSha256) {
    if (actualSha256.toLowerCase() !== expectedSha256.toLowerCase()) {
      throw new Error(`SHA-256 mismatch for ${asset}:\n  expected: ${expectedSha256}\n  actual:   ${actualSha256}`);
    }
    console.log(`SHA-256 OK: ${actualSha256}`);
  } else {
    console.warn(`No expected SHA-256 available; downloaded hash = ${actualSha256} (NOT verified).`);
  }

  const extractDir = path.join(tmpDir, 'extracted');
  fs.mkdirSync(extractDir, { recursive: true });
  if (ext === 'zip') {
    await extractZip(archivePath, extractDir);
  } else {
    await extractTarGz(archivePath, extractDir);
  }

  const entries = fs.readdirSync(extractDir, { withFileTypes: true });
  const innerDir = entries.find((e) => e.isDirectory());
  const srcDir = innerDir ? path.join(extractDir, innerDir.name) : extractDir;

  fs.mkdirSync(binDir, { recursive: true });
  if (process.platform === 'win32') {
    fs.mkdirSync(dllDir, { recursive: true });
  }

  // Copy sidecars
  const rawScrcpy = path.join(srcDir, `scrcpy${exeExt}`);
  const rawAdb = path.join(srcDir, `adb${exeExt}`);
  const rawServer = path.join(srcDir, 'scrcpy-server');

  if (fs.existsSync(rawScrcpy)) {
    fs.copyFileSync(rawScrcpy, scrcpySidecar);
    fs.chmodSync(scrcpySidecar, 0o755);
  }
  if (fs.existsSync(rawAdb)) {
    fs.copyFileSync(rawAdb, adbSidecar);
    fs.chmodSync(adbSidecar, 0o755);
  }
  if (fs.existsSync(rawServer)) {
    fs.copyFileSync(rawServer, serverDst);
  }

  // Copy DLLs on Windows
  if (fs.existsSync(srcDir)) {
    for (const file of fs.readdirSync(srcDir)) {
      if (file.endsWith('.dll')) {
        fs.copyFileSync(path.join(srcDir, file), path.join(dllDir, file));
      } else if (file.endsWith('.png')) {
        fs.copyFileSync(path.join(srcDir, file), path.join(binDir, file));
      }
    }
  }

  // Cleanup tmp dir
  try {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  } catch {
    // ignore
  }

  console.log('\nDone. Staged binaries in src-tauri/binaries:');
  for (const f of fs.readdirSync(binDir)) {
    console.log(`  ${f}`);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
