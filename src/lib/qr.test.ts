import { describe, it, expect } from "vitest";
import { generatePairingChallenge } from "./qr";

describe("generatePairingChallenge", () => {
  it("generates a challenge matching the adb-tls-pairing schema", () => {
    const challenge = generatePairingChallenge();

    // Name format: adbqr-<8 hex chars>
    expect(challenge.name).toMatch(/^adbqr-[0-9a-f]{8}$/);

    // Password format: 12 alphanumeric characters
    expect(challenge.password).toHaveLength(12);
    expect(challenge.password).toMatch(/^[A-Za-z0-9]{12}$/);

    // Payload format: Android Studio WIFI:T:ADB;S:<name>;P:<password>;;
    expect(challenge.payload).toBe(
      `WIFI:T:ADB;S:${challenge.name};P:${challenge.password};;`,
    );
  });

  it("produces unique challenges across successive calls", () => {
    const first = generatePairingChallenge();
    const second = generatePairingChallenge();

    expect(first.name).not.toBe(second.name);
    expect(first.password).not.toBe(second.password);
    expect(first.payload).not.toBe(second.payload);
  });
});
