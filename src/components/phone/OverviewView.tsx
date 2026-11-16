import { useCallback, useEffect, useState } from "react";
import {
  Battery,
  BatteryCharging,
  HardDrive,
  Headphones,
  Lock,
  Moon,
  Play,
  RefreshCw,
  Signal,
  Smartphone,
  Square,
  Sun,
  TriangleAlert,
  Volume2,
} from "lucide-react";
import {
  getPhoneToggles,
  setPhoneToggle,
  startMirror,
  stopMirror,
} from "@/lib/tauri";
import type { PhoneToggleKey, PhoneToggles } from "@/lib/types";
import {
  phoneAudioStart,
  phoneAudioStop,
  phoneGetOverview,
} from "@/services/phoneHubService";
import { useAppStore } from "@/store/useAppStore";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import { logActivity } from "@/stores/activityStore";
import type { PhoneOverview, PhoneScreenState } from "@/types/phone";

interface OverviewViewProps {
  serial: string;
  isWireless: boolean;
}

const Nil = () => (
  <span className="text-text-muted" aria-label="Not available">
    —
  </span>
);

function formatGb(bytes: number): string {
  const gb = bytes / (1024 * 1024 * 1024);
  return `${Math.round(gb * 10) / 10} GB`;
}

function formatUptime(secs: number): string {
  if (secs < 60) return `${secs} s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins} m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) {
    const remM = mins % 60;
    return remM > 0 ? `${hrs} h ${remM} m` : `${hrs} h`;
  }
  const days = Math.floor(hrs / 24);
  const remH = hrs % 24;
  return remH > 0 ? `${days} d ${remH} h` : `${days} d`;
}

function SignalBars({ level }: { level: number | null }) {
  if (level == null) return null;
  const clamped = Math.max(0, Math.min(4, Math.round(level)));
  const heights = ["h-1", "h-1.5", "h-2.5", "h-3.5"];
  return (
    <div className="mt-1 flex items-center justify-end gap-1.5">
      <span
        className="inline-flex h-3.5 items-end gap-0.5"
        role="img"
        aria-label={`Signal ${clamped} of 4`}
      >
        {[1, 2, 3, 4].map((i, idx) => (
          <i
            key={i}
            className={`w-1 rounded-[1px] ${heights[idx]} ${
              i <= clamped ? "bg-text-strong" : "bg-ink-700"
            }`}
          />
        ))}
      </span>
      <span className="text-[11px] text-text-muted">{clamped} of 4</span>
    </div>
  );
}

function ScreenChip({ screen }: { screen: PhoneScreenState | null }) {
  if (!screen) return <Nil />;
  const config: Record<PhoneScreenState, { label: string; Icon: typeof Sun }> = {
    on: { label: "On", Icon: Sun },
    off: { label: "Off", Icon: Moon },
    locked: { label: "On · locked", Icon: Lock },
  };
  const { label, Icon } = config[screen];
  return (
    <span className="inline-flex items-center gap-1 rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 font-mono text-[11px] text-text">
      <Icon className="h-3 w-3" />
      {label}
    </span>
  );
}

function ProgressBar({
  pct,
  tone = "mist",
}: {
  pct: number;
  tone?: "mist" | "gold" | "ok" | "warn";
}) {
  const clamped = Math.max(0, Math.min(100, pct));
  const fillClass =
    tone === "gold"
      ? "bg-gold-400"
      : tone === "ok"
        ? "bg-status-success"
        : tone === "warn"
          ? "bg-status-warning"
          : "bg-mist-300";

  return (
    <div
      className="h-1.5 overflow-hidden rounded-full border border-ink-700 bg-ink-800"
      role="progressbar"
      aria-valuenow={Math.round(clamped)}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <div className={`h-full ${fillClass}`} style={{ width: `${clamped}%` }} />
    </div>
  );
}

export function OverviewView({ serial, isWireless }: OverviewViewProps) {
  const overview: PhoneOverview | undefined = usePhoneHubStore(
    (s) => s.overviewsBySerial[serial],
  );
  const isStale = usePhoneHubStore((s) => Boolean(s.staleBySerial[serial]));
  const audioPlaying = usePhoneHubStore((s) =>
    Boolean(s.audioPlayingBySerial[serial]),
  );
  const applyOverview = usePhoneHubStore((s) => s.applyOverview);
  const applyAudioState = usePhoneHubStore((s) => s.applyAudioState);

  const sessions = useAppStore((s) => s.sessions);
  const settings = useAppStore((s) => s.settings);
  const activeSession = sessions.find((s) => s.serial === serial);

  const [toggles, setToggles] = useState<PhoneToggles | null>(null);
  const [togglingKey, setTogglingKey] = useState<PhoneToggleKey | null>(null);
  const [confirmWifiOff, setConfirmWifiOff] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [audioBusy, setAudioBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setConfirmWifiOff(false);
    setActionError(null);
    setToggles(null);

    getPhoneToggles(serial)
      .then((res) => {
        if (!cancelled) setToggles(res);
      })
      .catch(() => {
        if (!cancelled) setToggles(null);
      });

    return () => {
      cancelled = true;
    };
  }, [serial]);

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    setActionError(null);
    try {
      const next = await phoneGetOverview(serial, true);
      applyOverview(serial, next);
    } catch (e) {
      setActionError(String(e));
    } finally {
      setRefreshing(false);
    }
  }, [serial, applyOverview]);

  const handleMirror = useCallback(async () => {
    setActionError(null);
    try {
      if (activeSession) {
        await stopMirror(activeSession.id);
      } else {
        await startMirror(serial, settings);
      }
    } catch (e) {
      setActionError(String(e));
    }
  }, [activeSession, serial, settings]);

  const handleAudioToggle = useCallback(async () => {
    setAudioBusy(true);
    setActionError(null);
    try {
      if (audioPlaying) {
        await phoneAudioStop(serial);
        applyAudioState(serial, false);
      } else {
        await phoneAudioStart(serial);
        applyAudioState(serial, true);
      }
    } catch (e) {
      setActionError(String(e));
    } finally {
      setAudioBusy(false);
    }
  }, [audioPlaying, serial, applyAudioState]);

  const executeToggle = useCallback(
    async (key: PhoneToggleKey, nextValue: boolean) => {
      setTogglingKey(key);
      setActionError(null);
      try {
        await setPhoneToggle(serial, key, nextValue);
        setToggles((prev) => (prev ? { ...prev, [key]: nextValue } : prev));
        logActivity({
          kind: "device",
          title: `${key === "wifi" ? "Wi-Fi" : key === "bluetooth" ? "Bluetooth" : "Do Not Disturb"} ${nextValue ? "enabled" : "disabled"}`,
          detail: serial,
        });
      } catch (e) {
        setActionError(String(e));
      } finally {
        setTogglingKey(null);
      }
    },
    [serial],
  );

  const handleSwitchClick = useCallback(
    (key: PhoneToggleKey) => {
      if (!toggles || togglingKey !== null) return;
      const nextValue = !toggles[key];
      if (key === "wifi" && !nextValue && isWireless) {
        setConfirmWifiOff(true);
        return;
      }
      void executeToggle(key, nextValue);
    },
    [toggles, togglingKey, isWireless, executeToggle],
  );

  const battery = overview?.battery ?? null;
  const storage = overview?.storage ?? null;
  const ram = overview?.ram ?? null;
  const wifi = overview?.wifi ?? null;
  const cellular = overview?.cellular ?? null;

  const storagePct =
    storage && storage.totalBytes > 0
      ? (storage.usedBytes / storage.totalBytes) * 100
      : 0;
  const ramPct =
    ram && ram.totalBytes > 0 ? (ram.usedBytes / ram.totalBytes) * 100 : 0;

  const switchItems: Array<{ key: PhoneToggleKey; label: string }> = [
    { key: "wifi", label: "Wi-Fi" },
    { key: "bluetooth", label: "Bluetooth" },
    { key: "dnd", label: "Do Not Disturb" },
  ];

  return (
    <div className="p-4">
      <div className="space-y-4">
        {isStale && (
          <div
            role="status"
            className="flex items-center gap-2 rounded-lg border border-status-warning/40 bg-status-warning-bg px-3 py-2 text-xs text-status-warning"
          >
            <TriangleAlert className="h-3.5 w-3.5 shrink-0" />
            <span>
              Phone disconnected — showing last known overview until it reconnects.
            </span>
          </div>
        )}

        {actionError && (
          <div
            role="alert"
            className="rounded-lg border border-status-danger/40 bg-status-danger-bg px-3 py-2 text-xs text-status-danger"
          >
            {actionError}
          </div>
        )}

        {/* Quick actions card */}
        <section className="rounded-xl border border-ink-700 bg-ink-900 p-3.5">
          <div className="flex flex-wrap items-center gap-3">
            <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
              Quick actions
            </span>

            <button
              type="button"
              onClick={() => void handleMirror()}
              className={`btn-tactile inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-xs font-semibold cursor-pointer ${
                activeSession
                  ? "border-status-danger-solid bg-status-danger-solid text-text-strong"
                  : "border-gold-400 bg-gold-400 text-ink-950 hover:border-gold-600 hover:bg-gold-600"
              }`}
            >
              {activeSession ? (
                <>
                  <Square className="h-3 w-3" />
                  Stop mirror
                </>
              ) : (
                <>
                  <Play className="h-3 w-3" />
                  Mirror
                </>
              )}
            </button>

            <button
              type="button"
              disabled={audioBusy}
              aria-pressed={audioPlaying}
              onClick={() => void handleAudioToggle()}
              className={`btn-tactile inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-xs font-medium cursor-pointer disabled:opacity-50 ${
                audioPlaying
                  ? "border-gold-400 bg-ink-800 text-gold-400"
                  : "border-ink-700 bg-ink-800 text-text hover:border-text-muted hover:text-text-strong"
              }`}
            >
              <Headphones className="h-3.5 w-3.5" />
              {audioPlaying ? "Stop audio" : "Audio only"}
            </button>

            <div className="flex-1" />

            <div className="flex flex-wrap items-center gap-2">
              {switchItems.map(({ key, label }) => {
                const checked = toggles ? toggles[key] : false;
                const disabled = !toggles || togglingKey !== null;
                return (
                  <div
                    key={key}
                    className="flex items-center gap-2 rounded-lg border border-ink-700 bg-ink-950 px-2.5 py-1"
                  >
                    <span className="text-xs font-medium text-text-strong">
                      {label}
                    </span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={checked}
                      aria-label={label}
                      disabled={disabled}
                      onClick={() => handleSwitchClick(key)}
                      className={`btn-tactile relative inline-block h-[18px] w-8 shrink-0 rounded-full border-2 border-transparent transition-colors duration-150 ease-out cursor-pointer disabled:opacity-50 ${
                        checked
                          ? "bg-gold-400"
                          : "bg-ink-800 ring-1 ring-inset ring-ink-700"
                      }`}
                    >
                      <span
                        className={`block h-3.5 w-3.5 rounded-full transition-transform duration-150 ease-out ${
                          checked
                            ? "translate-x-3.5 bg-ink-950"
                            : "translate-x-0 bg-text-muted"
                        }`}
                      />
                    </button>
                  </div>
                );
              })}
            </div>
          </div>

          {audioPlaying && (
            <div className="mt-3 flex items-center gap-2 rounded-lg border border-ink-700 bg-ink-950 px-3 py-2 text-xs text-text">
              <Volume2 className="h-4 w-4 shrink-0 text-gold-400" />
              <span>Playing phone audio on this PC. No mirror window is open.</span>
            </div>
          )}

          {confirmWifiOff && (
            <div
              role="alert"
              className="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-lg border border-status-warning/45 bg-status-warning-bg px-3 py-2 text-xs text-status-warning"
            >
              <span className="flex items-center gap-2">
                <TriangleAlert className="h-3.5 w-3.5 shrink-0" />
                This phone is connected over Wi-Fi. Turning Wi-Fi off will disconnect it from AnyLeap.
              </span>
              <span className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => {
                    setConfirmWifiOff(false);
                    void executeToggle("wifi", false);
                  }}
                  className="btn-tactile rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-[11px] font-medium text-text-strong hover:border-text-muted cursor-pointer"
                >
                  Turn off &amp; disconnect
                </button>
                <button
                  type="button"
                  autoFocus
                  onClick={() => setConfirmWifiOff(false)}
                  className="btn-tactile rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-[11px] font-medium text-text-strong hover:border-text-muted cursor-pointer"
                >
                  Keep on
                </button>
              </span>
            </div>
          )}
        </section>

        {/* 2x2 Status Cards (Variant A) */}
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          {/* Battery */}
          <section
            aria-labelledby="ov-battery"
            className="rounded-xl border border-ink-700 bg-ink-900"
          >
            <div className="flex items-center justify-between gap-2 border-b border-ink-700 px-3.5 py-2.5">
              <h2
                id="ov-battery"
                className="flex items-center gap-1.5 text-xs font-semibold text-text-strong"
              >
                <Battery className="h-3.5 w-3.5" />
                Battery
              </h2>
              {battery &&
                (battery.charging ? (
                  <span className="inline-flex items-center gap-1 rounded bg-status-success-bg px-1.5 py-0.5 font-mono text-[11px] text-status-success">
                    <BatteryCharging className="h-3 w-3" />
                    Charging
                  </span>
                ) : (
                  <span className="inline-flex items-center rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 font-mono text-[11px] text-text">
                    Not charging
                  </span>
                ))}
            </div>
            <div className="p-3.5">
              {battery ? (
                <>
                  <div className="text-[32px] font-bold leading-9 text-text-strong tabular-nums">
                    {battery.level}%
                  </div>
                  <div className="my-3">
                    <ProgressBar
                      pct={battery.level}
                      tone={
                        battery.charging
                          ? "ok"
                          : battery.level <= 20
                            ? "warn"
                            : "mist"
                      }
                    />
                  </div>
                  <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-xs">
                    <dt className="text-text-muted">Temperature</dt>
                    <dd className="text-right text-text-strong tabular-nums">
                      {battery.tempC != null ? (
                        `${battery.tempC.toFixed(1)} °C`
                      ) : (
                        <Nil />
                      )}
                    </dd>
                    <dt className="text-text-muted">Health</dt>
                    <dd className="text-right text-text-strong">
                      {battery.health ?? <Nil />}
                    </dd>
                  </dl>
                </>
              ) : (
                <>
                  <div className="text-[32px] font-bold leading-9 text-text-strong">
                    <Nil />
                  </div>
                  <p className="mt-1.5 text-xs text-text-muted">
                    Battery details unavailable.
                  </p>
                </>
              )}
            </div>
          </section>

          {/* Storage & memory */}
          <section
            aria-labelledby="ov-storage"
            className="rounded-xl border border-ink-700 bg-ink-900"
          >
            <div className="flex items-center justify-between gap-2 border-b border-ink-700 px-3.5 py-2.5">
              <h2
                id="ov-storage"
                className="flex items-center gap-1.5 text-xs font-semibold text-text-strong"
              >
                <HardDrive className="h-3.5 w-3.5" />
                Storage &amp; memory
              </h2>
            </div>
            <div className="p-3.5">
              <div className="flex items-center justify-between text-xs">
                <span className="text-text-muted">Storage</span>
                <span className="font-medium text-text-strong tabular-nums">
                  {storage ? (
                    `${formatGb(storage.usedBytes)} of ${formatGb(storage.totalBytes)}`
                  ) : (
                    <Nil />
                  )}
                </span>
              </div>
              <div className="mt-1.5 mb-3.5">
                <ProgressBar
                  pct={storage ? storagePct : 0}
                  tone={storage && storagePct > 90 ? "warn" : "gold"}
                />
              </div>

              <div className="flex items-center justify-between text-xs">
                <span className="text-text-muted">RAM</span>
                <span className="font-medium text-text-strong tabular-nums">
                  {ram ? (
                    `${formatGb(ram.usedBytes)} of ${formatGb(ram.totalBytes)}`
                  ) : (
                    <Nil />
                  )}
                </span>
              </div>
              <div className="mt-1.5">
                <ProgressBar pct={ram ? ramPct : 0} tone="gold" />
              </div>
            </div>
          </section>

          {/* Connectivity */}
          <section
            aria-labelledby="ov-connectivity"
            className="rounded-xl border border-ink-700 bg-ink-900"
          >
            <div className="flex items-center justify-between gap-2 border-b border-ink-700 px-3.5 py-2.5">
              <h2
                id="ov-connectivity"
                className="flex items-center gap-1.5 text-xs font-semibold text-text-strong"
              >
                <Signal className="h-3.5 w-3.5" />
                Connectivity
              </h2>
            </div>
            <div className="p-3.5">
              <dl className="divide-y divide-ink-700 text-xs">
                <div className="flex items-start justify-between gap-4 pb-2.5">
                  <dt className="text-text-muted">Wi-Fi</dt>
                  <dd className="text-right text-text-strong">
                    {wifi ? (
                      wifi.ssid ? (
                        wifi.ssid
                      ) : (
                        <span title="Android hides the network name on this phone">
                          Wi-Fi (name hidden)
                        </span>
                      )
                    ) : (
                      <Nil />
                    )}
                    {wifi && <SignalBars level={wifi.signalLevel} />}
                  </dd>
                </div>
                <div className="flex items-start justify-between gap-4 pt-2.5">
                  <dt className="text-text-muted">Cellular</dt>
                  <dd className="text-right text-text-strong">
                    {cellular?.operator ?? <Nil />}
                    {cellular && <SignalBars level={cellular.signalLevel} />}
                  </dd>
                </div>
              </dl>
            </div>
          </section>

          {/* Device */}
          <section
            aria-labelledby="ov-device"
            className="rounded-xl border border-ink-700 bg-ink-900"
          >
            <div className="flex items-center justify-between gap-2 border-b border-ink-700 px-3.5 py-2.5">
              <h2
                id="ov-device"
                className="flex items-center gap-1.5 text-xs font-semibold text-text-strong"
              >
                <Smartphone className="h-3.5 w-3.5" />
                Device
              </h2>
            </div>
            <div className="p-3.5">
              <dl className="divide-y divide-ink-700 text-xs">
                <div className="flex items-center justify-between gap-4 py-2 first:pt-0">
                  <dt className="text-text-muted">Model</dt>
                  <dd className="text-right text-text-strong">
                    {overview?.model ?? <Nil />}
                  </dd>
                </div>
                <div className="flex items-center justify-between gap-4 py-2">
                  <dt className="text-text-muted">Brand</dt>
                  <dd className="text-right text-text-strong">
                    {overview?.brand ?? <Nil />}
                  </dd>
                </div>
                <div className="flex items-center justify-between gap-4 py-2">
                  <dt className="text-text-muted">Android</dt>
                  <dd className="text-right text-text-strong">
                    {overview?.androidVersion ?? <Nil />}
                  </dd>
                </div>
                <div className="flex items-center justify-between gap-4 py-2">
                  <dt className="text-text-muted">Security patch</dt>
                  <dd className="text-right text-text-strong">
                    {overview?.securityPatch ?? <Nil />}
                  </dd>
                </div>
                <div className="flex items-center justify-between gap-4 py-2">
                  <dt className="text-text-muted">Screen</dt>
                  <dd className="text-right text-text-strong">
                    <ScreenChip screen={overview?.screen ?? null} />
                  </dd>
                </div>
                <div className="flex items-center justify-between gap-4 py-2 last:pb-0">
                  <dt className="text-text-muted">Uptime</dt>
                  <dd className="text-right text-text-strong tabular-nums">
                    {overview?.uptimeSecs != null ? (
                      formatUptime(overview.uptimeSecs)
                    ) : (
                      <Nil />
                    )}
                  </dd>
                </div>
              </dl>
            </div>
          </section>
        </div>

        {/* Footer */}
        <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
          <span className="text-text-muted">
            Fast values refresh every 30 s while this tab is open; storage and RAM every 5 min.
          </span>
          <button
            type="button"
            disabled={refreshing}
            onClick={() => void handleRefresh()}
            className="btn-tactile inline-flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-[11px] font-medium text-text hover:border-text-muted hover:text-text-strong disabled:opacity-50 cursor-pointer"
          >
            <RefreshCw className={`h-3 w-3 ${refreshing ? "animate-spin" : ""}`} />
            Refresh now
          </button>
        </div>
      </div>
    </div>
  );
}
