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

const VERSION = process.env.SCRCPY_VERSION || '5.0.1';
const FORCE = process.argv.includes('--force') || process.argv.includes('-f');

const KNOWN_SHA256 = {
  '5.0.1': {
    'scrcpy-win64-v5.0.1.zip': 'b12a2c4ee8be317422451fc7dcf8ee20a71b5ea7ef9ad73ddd825a25316227a5',
    'scrcpy-win32-v5.0.1.zip': '0cb65ebf1bce892fa4d1502d699af0c7a1a7f24d7febcb9ef72894e3f23fc2ad',
    'scrcpy-winarm64-v5.0.1.zip': '16c3fc2068df64946f670e3504b4752073ebae47a416a45b46e79a5ab19c689b',
    'scrcpy-macos-aarch64-v5.0.1.tar.gz': '33611e51977a8289e2e124b220f727895181ef2eb9060e0987a4dc2db9cf0d6b',
    'scrcpy-macos-x86_64-v5.0.1.tar.gz': '31a5467a9e907f162b093267cbbab1276b15c6e709c2ea8159566a8d165beb50',
    'scrcpy-linux-x86_64-v5.0.1.tar.gz': '9f969d30cc574816077edecda65719c1c70b0aee1df1ba5fa00779ac07b8fd6e',
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
    let asset = `scrcpy-win64-v${VERSION}.zip`;
    if (arch === 'ia32') {
      asset = `scrcpy-win32-v${VERSION}.zip`;
    } else if (arch === 'arm64') {
      asset = `scrcpy-winarm64-v${VERSION}.zip`;
    }
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
  // Windows 10+ ships bsdtar, which reads zip. Expand-Archive breaks when a pwsh 7
  // PSModulePath leaks into Windows PowerShell, and it does not fail the build.
  if (process.platform === 'win32') {
    // Call it by full path so a GNU tar from Git Bash on PATH doesn't shadow it.
    const tar = path.join(process.env.SystemRoot || 'C:/Windows', 'System32', 'tar.exe');
    execSync(`"${tar}" -xf "${zipPath}" -C "${targetDir}"`, { stdio: 'inherit' });
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
  const versionFile = path.join(binDir, '.scrcpy-version');
  const currentVersion = fs.existsSync(versionFile) ? fs.readFileSync(versionFile, 'utf8').trim() : '';

  if (!FORCE && currentVersion === VERSION && fs.existsSync(scrcpySidecar) && fs.existsSync(adbSidecar) && fs.existsSync(serverDst)) {
    console.log(`Binaries already present for v${VERSION} (use --force to re-download). Skipping.`);
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

  // Remove stale DLLs from previous versions (e.g. FFmpeg ABI bumps) before copying
  if (process.platform === 'win32' && fs.existsSync(dllDir)) {
    for (const file of fs.readdirSync(dllDir)) {
      if (file.endsWith('.dll')) {
        fs.unlinkSync(path.join(dllDir, file));
      }
    }
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

  fs.writeFileSync(versionFile, `${VERSION}\n`);

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
