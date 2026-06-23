// Android-Studio-style QR pairing challenge for "Pair device with QR code".
// The phone scans the payload, then advertises an `_adb-tls-pairing._tcp`
// service named `name`; we discover it and `adb pair` with `password`.

export interface PairingChallenge {
  name: string;
  password: string;
  payload: string; // "WIFI:T:ADB;S:<name>;P:<password>;;"
}

function randomHex(bytes: number): string {
  const buf = new Uint8Array(bytes);
  crypto.getRandomValues(buf);
  return Array.from(buf, (b) => b.toString(16).padStart(2, "0")).join("");
}

function randomPassword(length: number): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  const buf = new Uint8Array(length);
  crypto.getRandomValues(buf);
  return Array.from(buf, (b) => alphabet[b % alphabet.length]).join("");
}

/** Build a fresh QR pairing challenge (name + password + QR payload string). */
export function generatePairingChallenge(): PairingChallenge {
  const name = `adbqr-${randomHex(4)}`;
  const password = randomPassword(12);
  const payload = `WIFI:T:ADB;S:${name};P:${password};;`;
  return { name, password, payload };
}
