// QR pairing session: listens for the pairing service a phone advertises after
// scanning, regenerates the challenge when the listen window expires, and calls
// `pair` exactly once per challenge. A QR is only exposed while listening.
import { useEffect, useRef, useState } from "react";
import { generatePairingChallenge, type PairingChallenge } from "@/lib/qr";
import type { MdnsService } from "@/lib/types";

export const LISTEN_WINDOW_MS = 120_000;
export const OVERALL_CAP_MS = 600_000;
export const POLL_MS = 800;
/** Previous challenge stays valid this long after its window ends (phone scanned just before regeneration). */
export const GRACE_MS = 60_000;

export type QrPhase = "listening" | "pairing" | "paired" | "failed" | "expired" | "cancelled";

export interface QrSessionState {
  phase: QrPhase;
  /** Challenge to render as QR. Non-null ONLY while phase === "listening". */
  challenge: PairingChallenge | null;
  secondsLeft: number;
  error: string | null;
}

export interface QrPaired {
  pairing: MdnsService;
  /** Connect endpoint if already advertised in the same discovery round. */
  connect?: MdnsService;
}

export interface QrSessionDeps {
  discover: () => Promise<MdnsService[]>;
  pair: (host: string, port: number, password: string) => Promise<unknown>;
  generate?: () => PairingChallenge;
  onPaired?: (r: QrPaired) => void;
  listenWindowMs?: number;
  overallCapMs?: number;
  pollMs?: number;
  graceMs?: number;
}

export function createQrPairingSession(deps: QrSessionDeps) {
  const generate = deps.generate ?? generatePairingChallenge;
  const windowMs = deps.listenWindowMs ?? LISTEN_WINDOW_MS;
  const capMs = deps.overallCapMs ?? OVERALL_CAP_MS;
  const pollMs = deps.pollMs ?? POLL_MS;
  const graceMs = deps.graceMs ?? GRACE_MS;

  let state: QrSessionState = { phase: "cancelled", challenge: null, secondsLeft: 0, error: null };
  const listeners = new Set<() => void>();
  let gen = 0; // bumps on every start/cancel to invalidate stale loops
  let timer: ReturnType<typeof setTimeout> | null = null;
  let tick: ReturnType<typeof setInterval> | null = null;

  const set = (p: Partial<QrSessionState>) => {
    state = { ...state, ...p };
    listeners.forEach((l) => l());
  };
  const clear = () => {
    if (timer) clearTimeout(timer);
    if (tick) clearInterval(tick);
    timer = tick = null;
  };

  function start() {
    clear();
    const my = ++gen;
    const sessionEnd = Date.now() + capMs;
    const issued: { challenge: PairingChallenge; validUntil: number }[] = [];
    const begin = () => {
      const challenge = generate();
      const windowEnd = Math.min(Date.now() + windowMs, sessionEnd);
      issued.push({ challenge, validUntil: windowEnd + graceMs });
      let paired = false;
      const left = () => Math.max(0, Math.ceil((windowEnd - Date.now()) / 1000));
      set({ phase: "listening", challenge, secondsLeft: left(), error: null });
      clear();
      tick = setInterval(() => {
        if (my === gen && state.phase === "listening") set({ secondsLeft: left() });
      }, 1000);
      const poll = async () => {
        if (my !== gen || paired) return;
        if (Date.now() >= windowEnd) {
          clear();
          if (Date.now() >= sessionEnd) {
            set({ phase: "expired", challenge: null, secondsLeft: 0 });
          } else {
            begin();
          }
          return;
        }
        let found: MdnsService[] = [];
        try {
          found = await deps.discover();
        } catch {
          found = [];
        }
        if (my !== gen || paired) return;
        const now = Date.now();
        let used: PairingChallenge | undefined;
        const pairing = found.find((s) => {
          if (!s.serviceType.includes("pairing")) return false;
          const hit = issued.find((i) => i.validUntil > now && i.challenge.name === s.name);
          if (hit) used = hit.challenge;
          return !!hit;
        });
        if (pairing && used) {
          const password = (used as PairingChallenge).password;
          paired = true;
          clear();
          const connect = found.find(
            (s) => s.host === pairing.host && s.serviceType.includes("connect"),
          );
          set({ phase: "pairing", challenge: null, secondsLeft: 0 });
          try {
            await deps.pair(pairing.host, pairing.port, password);
          } catch (e) {
            if (my !== gen) return;
            set({ phase: "failed", error: String(e) });
            return;
          }
          if (my !== gen) return;
          set({ phase: "paired" });
          deps.onPaired?.({ pairing, connect });
          return;
        }
        timer = setTimeout(poll, pollMs);
      };
      void poll();
    };
    begin();
  }

  function cancel() {
    gen++;
    clear();
    set({ phase: "cancelled", challenge: null, secondsLeft: 0 });
  }

  return {
    start,
    cancel,
    retry: start,
    getState: () => state,
    subscribe(l: () => void) {
      listeners.add(l);
      return () => listeners.delete(l);
    },
  };
}

export function useQrPairingSession(active: boolean, deps: QrSessionDeps): QrSessionState & {
  restart: () => void;
} {
  const depsRef = useRef(deps);
  depsRef.current = deps;
  const [session] = useState(() =>
    createQrPairingSession({
      discover: () => depsRef.current.discover(),
      pair: (h, p, pw) => depsRef.current.pair(h, p, pw),
      onPaired: (r) => depsRef.current.onPaired?.(r),
    }),
  );
  const [state, setState] = useState(session.getState());
  useEffect(() => {
    const off = session.subscribe(() => setState(session.getState()));
    return () => void off();
  }, [session]);
  useEffect(() => {
    if (!active) return;
    session.start();
    return () => session.cancel();
  }, [active, session]);
  return { ...state, restart: session.retry };
}
