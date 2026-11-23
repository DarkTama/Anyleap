/**
 * Default transfer folder settings & validation (SPEC-31).
 */

export interface TransferFoldersSettings {
  /** null = use OS default (<Downloads>/AnyLeap). Non-null = user-chosen absolute path. */
  receiveDir: string | null;
  /** Default false. Prompt on user-initiated PC downloads (Phone Hub, screenshot). */
  askWhereToSave: boolean;
  /** Android destination for mirror drops and pasted images. Default "/sdcard/Download". */
  phoneUploadDir: string;
}

export const DEFAULT_PHONE_UPLOAD_DIR = "/sdcard/Download";

export const DEFAULT_TRANSFER_FOLDERS: TransferFoldersSettings = {
  receiveDir: null,
  askWhereToSave: false,
  phoneUploadDir: DEFAULT_PHONE_UPLOAD_DIR,
};

export const PHONE_UPLOAD_PRESETS = [
  { label: "Download", path: "/sdcard/Download" },
  { label: "Documents", path: "/sdcard/Documents" },
  { label: "Pictures", path: "/sdcard/Pictures" },
] as const;

export const PHONE_UPLOAD_DIR_ERROR =
  "Enter a folder inside /sdcard or /storage/emulated/0 (for example /sdcard/Download).";

const FORBIDDEN_PHONE_CHARS = /[\u0000-\u001f\u007f'"`$;&|<>\\*?]/;

export function normalizePhoneUploadDir(
  raw: string,
): { ok: true; path: string } | { ok: false; error: string } {
  if (FORBIDDEN_PHONE_CHARS.test(raw)) {
    return { ok: false, error: PHONE_UPLOAD_DIR_ERROR };
  }
  const trimmed = raw.trim();
  if (!trimmed || !trimmed.startsWith("/")) {
    return { ok: false, error: PHONE_UPLOAD_DIR_ERROR };
  }

  const segments: string[] = [];
  for (const seg of trimmed.split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") {
      return { ok: false, error: PHONE_UPLOAD_DIR_ERROR };
    }
    segments.push(seg);
  }

  if (segments.length === 0) {
    return { ok: false, error: PHONE_UPLOAD_DIR_ERROR };
  }

  const normalized = `/${segments.join("/")}`;
  const underSdcard = normalized === "/sdcard" || normalized.startsWith("/sdcard/");
  const underEmulated =
    normalized === "/storage/emulated/0" ||
    normalized.startsWith("/storage/emulated/0/");

  if (!underSdcard && !underEmulated) {
    return { ok: false, error: PHONE_UPLOAD_DIR_ERROR };
  }

  return { ok: true, path: normalized };
}

export function validatePhoneUploadDir(raw: string): string | null {
  const res = normalizePhoneUploadDir(raw);
  return res.ok ? null : res.error;
}

export function normalizeTransferFolders(raw: unknown): TransferFoldersSettings {
  if (!raw || typeof raw !== "object") {
    return { ...DEFAULT_TRANSFER_FOLDERS };
  }
  const obj = raw as Record<string, unknown>;
  const receiveDir =
    typeof obj.receiveDir === "string" && obj.receiveDir.trim().length > 0
      ? obj.receiveDir.trim()
      : null;
  const askWhereToSave = obj.askWhereToSave === true;

  let phoneUploadDir = DEFAULT_PHONE_UPLOAD_DIR;
  if (typeof obj.phoneUploadDir === "string") {
    const res = normalizePhoneUploadDir(obj.phoneUploadDir);
    if (res.ok) phoneUploadDir = res.path;
  }

  return {
    receiveDir,
    askWhereToSave,
    phoneUploadDir,
  };
}

export function resolveDisplayReceiveDir(
  receiveDir: string | null,
  defaultReceiveDir: string,
): string {
  return receiveDir || defaultReceiveDir || "Downloads\\AnyLeap";
}
