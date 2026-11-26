import { useEffect, useId, useState, type ReactNode } from "react";
import { LayoutGrid, Monitor } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { selectClass, optionClass } from "@/lib/styles";
import { getScrollMultiplier, getSystemResolution } from "@/lib/tauri";
import { useAppStore } from "@/store/useAppStore";
import { dismissModeNotice, editMode, launchMode } from "@/lib/modeActions";
import {
  BITRATE_VALUE,
  CAMERA_BITRATE_BPS,
  QUALITY_LABELS,
  RESOLUTION_VALUE,
  SCROLL_SPEED_OPTIONS,
  TOUCHPAD_SPEED_OPTIONS,
  applyQuality,
  deriveQuality,
  formatScrollMultiplier,
  segmentForBitrate,
  segmentForFps,
  segmentForResolution,
  type BitrateSegment,
  type DisplayMode,
  type MirrorMode,
  type ModeSettings,
  type QualityChoice,
  type ResolutionSegment,
  type ScrollStyle,
} from "@/lib/modeSettings";
import type { Codec, CoreSettings } from "@/lib/types";
import { entry } from "./registry";
import { ModeTabs } from "./ModeTabs";
import { SegmentedControl } from "./SegmentedControl";
import { SwitchRow } from "./SwitchRow";

type QualityPick = Exclude<QualityChoice, "custom">;
const QUALITY_SEGMENTS: { value: QualityPick; label: string }[] = [
  { value: "smooth", label: QUALITY_LABELS.smooth },
  { value: "balanced", label: QUALITY_LABELS.balanced },
  { value: "sharp", label: QUALITY_LABELS.sharp },
  { value: "maximum", label: QUALITY_LABELS.maximum },
];
const RESOLUTION_SEGMENTS: { value: Exclude<ResolutionSegment, "custom">; label: string }[] = [
  { value: "native", label: "Native" },
  { value: "1080p", label: "1080p" },
  { value: "720p", label: "720p" },
];
const BITRATE_SEGMENTS: { value: BitrateSegment; label: string }[] = [
  { value: "low", label: "Low" },
  { value: "balanced", label: "Balanced" },
  { value: "high", label: "High" },
];
const FPS_SEGMENTS: { value: 30 | 60; label: string }[] = [
  { value: 30, label: "30 fps" },
  { value: 60, label: "60 fps" },
];
const CODEC_LABELS: Record<Codec, string> = { h264: "H.264", h265: "H.265", av1: "AV1" };
const FLEX_SIZE_PRESETS = [
  { label: "1080p compact (160 dpi)", val: "1920x1080/160" },
  { label: "1080p balanced (200 dpi)", val: "1920x1080/200" },
  { label: "1080p standard (240 dpi)", val: "1920x1080/240" },
  { label: "1440p HiDPI (200 dpi)", val: "2560x1440/200" },
];
const FIT_LABELS: Record<CoreSettings["renderFit"], string> = {
  letterbox: "Letterbox",
  unscaled: "Unscaled",
  stretched: "Stretched",
};
const SCROLL_STYLE_SEGMENTS: { value: ScrollStyle; label: string }[] = [
  { value: "smooth", label: "Smooth (reading)" },
  { value: "paged", label: "Paged (short videos)" },
];

const mbps = (bps: number) => `${Math.round(bps / 1_000_000)} Mbps`;

function Group({ title, children }: { title: string; children: ReactNode }) {
  const id = useId();
  return (
    <section
      aria-labelledby={id}
      className="space-y-1 border-t border-ink-700 pt-3 first:border-t-0 first:pt-0"
    >
      <h4
        id={id}
        className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted"
      >
        {title}
      </h4>
      {children}
    </section>
  );
}

/** Quality, Audio & control, Phone while mirroring, Window options (+ Flex display on Flex). */
function DisplayModePanel({ mode }: { mode: DisplayMode }) {
  const s: ModeSettings = useAppStore((st) => st.modeDefaults[mode]);
  const [fineOpen, setFineOpen] = useState(false);
  const fineId = useId();
  const quality = deriveQuality(s);
  const custom = quality === "custom";
  const resSeg = segmentForResolution(s.maxSize);
  const fpsSeg = segmentForFps(s.maxFps);
  const bitSeg = segmentForBitrate(s.videoBitRate);
  const set = (patch: Partial<ModeSettings>) => editMode(mode, patch);

  return (
    <div className="space-y-4">
      <Group title="Quality">
        <SegmentedControl<QualityPick>
          label="Quality"
          hint="How good is your connection? Smooth saves bandwidth; Maximum needs USB or strong Wi-Fi."
          segments={QUALITY_SEGMENTS}
          value={custom ? null : (quality as QualityPick)}
          customLabel={custom ? "Custom" : undefined}
          onChange={(choice) => {
            const patch = applyQuality(choice);
            if (patch) set(patch);
          }}
        />
        <button
          type="button"
          aria-expanded={custom || fineOpen}
          aria-controls={fineId}
          disabled={custom}
          onClick={() => setFineOpen((v) => !v)}
          className="min-h-8 cursor-pointer text-xs font-medium text-gold-400 underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400 disabled:cursor-default disabled:no-underline"
        >
          {custom ? "Fine-tune (Custom)" : fineOpen ? "Hide fine-tune" : "Fine-tune"}
        </button>
        <div id={fineId} hidden={!(custom || fineOpen)} className="space-y-2 pl-0.5">
          <SegmentedControl<Exclude<ResolutionSegment, "custom">>
            label="Resolution"
            hint="Long side of the video. Native is sharpest."
            segments={RESOLUTION_SEGMENTS}
            value={resSeg === "custom" ? null : resSeg}
            customLabel={resSeg === "custom" ? `Custom: ${s.maxSize} px` : undefined}
            onChange={(v) => set({ maxSize: RESOLUTION_VALUE[v] })}
          />
          <SegmentedControl<BitrateSegment>
            label="Bitrate"
            hint="Higher is sharper but needs a strong connection."
            segments={BITRATE_SEGMENTS}
            value={bitSeg}
            customLabel={mbps(s.videoBitRate)}
            onChange={(v) => set({ videoBitRate: BITRATE_VALUE[v] })}
          />
          <SegmentedControl<30 | 60>
            label="Frame rate"
            hint="60 is smoother; 30 saves bandwidth."
            segments={FPS_SEGMENTS}
            value={fpsSeg === "custom" ? null : fpsSeg}
            customLabel={fpsSeg === "custom" ? `Custom: ${s.maxFps} fps` : undefined}
            onChange={(v) => set({ maxFps: v })}
          />
          <div className="space-y-1 py-1">
            <label htmlFor={`${fineId}-codec`} className="text-sm text-text-strong">
              Codec
            </label>
            <select
              id={`${fineId}-codec`}
              aria-describedby={`${fineId}-codec-hint`}
              className={selectClass}
              value={s.videoCodec}
              onChange={(e) => set({ videoCodec: e.target.value as Codec })}
            >
              {(Object.keys(CODEC_LABELS) as Codec[]).map((c) => (
                <option key={c} value={c} className={optionClass}>
                  {CODEC_LABELS[c]}
                </option>
              ))}
            </select>
            <p id={`${fineId}-codec-hint`} className="text-[11px] text-text-muted">
              H.265 saves bandwidth. Needs a phone that supports it.
            </p>
          </div>
        </div>
      </Group>

      <Group title="Audio & control">
        <SwitchRow
          label="Play phone audio on this PC"
          hint="Off: sound stays on the phone."
          checked={!s.noAudio}
          onChange={(v) => set({ noAudio: !v })}
        />
        <SwitchRow
          label="Allow control from this PC"
          hint="Off: watch only."
          checked={!s.noControl}
          onChange={(v) => set({ noControl: !v })}
        />
        <SwitchRow
          label="Use Android on-screen keyboard"
          hint="Off: type with this PC's keyboard directly."
          checked={mode === "flex" ? false : !s.noKeyboardIme}
          disabled={mode === "flex"}
          disabledReason="Not used in Flex. PC typing works, and the phone's keyboard may appear on the phone while you type in Flex."
          onChange={(v) => set({ noKeyboardIme: !v })}
        />
      </Group>

      <Group title="Phone while mirroring">
        <SwitchRow
          label="Keep phone awake"
          hint="Stops the phone sleeping while mirroring."
          checked={s.stayAwake}
          onChange={(v) => set({ stayAwake: v })}
        />
        <SwitchRow
          label="Turn phone screen off"
          hint="Saves battery. Mirroring keeps working."
          checked={s.turnScreenOff}
          onChange={(v) => set({ turnScreenOff: v })}
        />
        <SwitchRow
          label="Show touches on the phone"
          hint="Draws a dot where a finger or click lands."
          checked={s.showTouches}
          onChange={(v) => set({ showTouches: v })}
        />
      </Group>

      <Group title="Window options">
        <SwitchRow
          label="Start in fullscreen"
          checked={s.fullscreen}
          onChange={(v) => set({ fullscreen: v })}
        />
        <SwitchRow
          label="Show phone in one AnyLeap window"
          hint="Keeps the phone view and its toolbar together."
          checked={s.embedded ?? false}
          onChange={(v) => set({ embedded: v })}
        />
        <SwitchRow
          label="Keep phone aspect ratio"
          hint="Off: the window can be resized to any shape."
          checked={!s.noWindowAspectRatioLock}
          disabled={s.embedded ?? false}
          disabledReason="Not available while the phone is shown in one AnyLeap window."
          onChange={(v) => set({ noWindowAspectRatioLock: !v })}
        />
        <PictureFit mode={mode} value={s.renderFit} />
      </Group>

      <ScrollingGroup mode={mode} s={s} />

      {mode === "flex" && <FlexDisplayGroup s={s} />}
    </div>
  );
}

function ScrollingGroup({ mode, s }: { mode: DisplayMode; s: ModeSettings }) {
  const id = useId();
  const sessions = useAppStore((st) => st.sessions);
  const sessionModes = useAppStore((st) => st.sessionModes);
  const currentMode = useAppStore((st) => st.currentMode);
  const runningSession = sessions.find(
    (sess) =>
      sess.mode !== "camera" && (sessionModes[sess.id] ?? launchMode(currentMode)) === mode,
  );
  const runningSerial = runningSession?.serial ?? null;
  const [liveMultiplier, setLiveMultiplier] = useState<number | null>(null);

  const isPaged = s.scroll.style === "paged";
  const isAuto = s.scroll.speed === "auto";

  useEffect(() => {
    if (!runningSerial || isPaged || !isAuto) {
      setLiveMultiplier(null);
      return;
    }
    let cancelled = false;
    const refresh = () => {
      getScrollMultiplier(runningSerial)
        .then((m) => {
          if (!cancelled) setLiveMultiplier(m);
        })
        .catch(() => {
          if (!cancelled) setLiveMultiplier(null);
        });
    };
    refresh();
    const timer = setInterval(refresh, 2000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [runningSerial, isPaged, isAuto]);

  const styleEntry = entry("M-21");
  const speedEntry = entry("M-22");
  const smoothEntry = entry("M-23");
  const touchpadEntry = entry("M-24");
  const speedHint = isPaged
    ? "Not used in Paged style."
    : isAuto
      ? runningSerial && liveMultiplier != null
        ? `Auto matches the phone's normal feel. Currently ${formatScrollMultiplier(liveMultiplier)}x on this screen.`
        : (speedEntry.helper ?? "Matches the phone's normal feel.")
      : "Scrolls at a fixed speed on every screen.";
  const touchpadHint = isPaged ? "Not used in Paged style." : touchpadEntry.helper;

  return (
    <Group title="Scrolling">
      <SegmentedControl<ScrollStyle>
        label={styleEntry.label}
        hint={styleEntry.helper}
        segments={SCROLL_STYLE_SEGMENTS}
        value={s.scroll.style}
        onChange={(style) => editMode(mode, { scroll: { ...s.scroll, style } })}
      />
      <div className="space-y-1 py-1">
        <label
          htmlFor={`${id}-speed`}
          className={`text-sm ${isPaged ? "text-text opacity-70" : "text-text-strong"}`}
        >
          {speedEntry.label}
        </label>
        <select
          id={`${id}-speed`}
          aria-describedby={`${id}-speed-hint`}
          disabled={isPaged}
          className={`${selectClass} disabled:cursor-not-allowed disabled:opacity-60`}
          value={isAuto ? "auto" : String(s.scroll.speed)}
          onChange={(e) => {
            const val = e.target.value;
            const speed = val === "auto" ? "auto" : Number.parseFloat(val);
            editMode(mode, { scroll: { ...s.scroll, speed } });
          }}
        >
          <option value="auto" className={optionClass}>
            Auto
          </option>
          {SCROLL_SPEED_OPTIONS.map((v) => (
            <option key={v} value={String(v)} className={optionClass}>
              {v}x
            </option>
          ))}
        </select>
        <p id={`${id}-speed-hint`} className="text-[11px] leading-snug text-text-muted">
          {speedHint}
        </p>
      </div>
      <div className="space-y-1 py-1">
        <label
          htmlFor={`${id}-touchpad-speed`}
          className={`text-sm ${isPaged ? "text-text opacity-70" : "text-text-strong"}`}
        >
          {touchpadEntry.label}
        </label>
        <select
          id={`${id}-touchpad-speed`}
          aria-describedby={`${id}-touchpad-speed-hint`}
          disabled={isPaged}
          className={`${selectClass} disabled:cursor-not-allowed disabled:opacity-60`}
          value={String(s.scroll.touchpadSpeed)}
          onChange={(e) => {
            const touchpadSpeed = Number.parseFloat(e.target.value);
            editMode(mode, { scroll: { ...s.scroll, touchpadSpeed } });
          }}
        >
          {TOUCHPAD_SPEED_OPTIONS.map((v) => (
            <option key={v} value={String(v)} className={optionClass}>
              {v}x
            </option>
          ))}
        </select>
        <p id={`${id}-touchpad-speed-hint`} className="text-[11px] leading-snug text-text-muted">
          {touchpadHint}
        </p>
      </div>
      <SwitchRow
        label={smoothEntry.label}
        hint={smoothEntry.helper}
        checked={s.scroll.smooth}
        disabled={isPaged}
        disabledReason="Not used in Paged style."
        onChange={(smooth) => editMode(mode, { scroll: { ...s.scroll, smooth } })}
      />
    </Group>
  );
}

function PictureFit({ mode, value }: { mode: DisplayMode; value: CoreSettings["renderFit"] }) {
  const id = useId();
  return (
    <div className="space-y-1 py-1">
      <label htmlFor={id} className="text-sm text-text-strong">
        Picture fit
      </label>
      <select
        id={id}
        aria-describedby={`${id}-hint`}
        className={selectClass}
        value={value}
        onChange={(e) => editMode(mode, { renderFit: e.target.value as CoreSettings["renderFit"] })}
      >
        {(Object.keys(FIT_LABELS) as CoreSettings["renderFit"][]).map((f) => (
          <option key={f} value={f} className={optionClass}>
            {FIT_LABELS[f]}
          </option>
        ))}
      </select>
      <p id={`${id}-hint`} className="text-[11px] text-text-muted">
        Letterbox keeps proportions. Stretched fills the window.
      </p>
    </div>
  );
}

function FlexDisplayGroup({ s }: { s: ModeSettings }) {
  const id = useId();
  const raw = s.flexDisplaySize || "";
  const dpiMatch = raw.match(/\/(\d+)/);
  const parsedDpi = dpiMatch ? Number.parseInt(dpiMatch[1], 10) : 240;
  const iconSize = Math.max(20, Math.min(48, Math.round(28 * (parsedDpi / 200))));
  const setSize = (v: string) => editMode("flex", { flexDisplaySize: v });
  const useThisPc = async () => {
    try {
      const [w, h] = await getSystemResolution();
      setSize(`${w}x${h}/200`);
    } catch (e) {
      console.error(e);
    }
  };
  return (
    <Group title="Flex display">
      <div className="space-y-1 py-1">
        <label htmlFor={`${id}-size`} className="text-sm text-text-strong">
          Virtual display size
        </label>
        <div className="flex gap-2">
          <input
            id={`${id}-size`}
            type="text"
            aria-describedby={`${id}-size-hint`}
            className={selectClass}
            value={s.flexDisplaySize}
            placeholder="Automatic"
            onChange={(e) => setSize(e.target.value)}
          />
          <button
            type="button"
            onClick={() => void useThisPc()}
            className="btn-tactile flex min-h-8 shrink-0 cursor-pointer items-center gap-1 rounded border border-ink-700 bg-ink-800 px-2 text-[11px] text-text hover:border-text-muted hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
          >
            <Monitor className="h-3 w-3" aria-hidden="true" />
            Use this PC&apos;s resolution
          </button>
        </div>
        <p id={`${id}-size-hint`} className="text-[11px] text-text-muted">
          Match this PC&apos;s monitor for a desktop-like layout. Format 1920x1080/240 (pixels /
          dpi). Empty means automatic. The button detects this PC&apos;s main monitor.
        </p>
      </div>

      <div className="flex items-center gap-3 rounded border border-ink-700 bg-ink-950 p-2.5">
        <div aria-hidden="true" className="flex shrink-0 flex-col items-center gap-1">
          <div
            className="flex items-center justify-center rounded-xl border border-gold-400/50 bg-ink-900 text-gold-400"
            style={{ width: `${iconSize}px`, height: `${iconSize}px` }}
          >
            <LayoutGrid className="h-1/2 w-1/2" />
          </div>
          <span className="text-[10px] text-text-muted">App</span>
        </div>
        <div className="text-xs text-text">
          <span className="font-semibold text-text-strong">{parsedDpi} dpi</span> icon size preview
          <p className="mt-0.5 text-[11px] text-text-muted">
            {parsedDpi <= 160
              ? "Compact: small icons, maximum desktop workspace"
              : parsedDpi <= 200
                ? "Medium: balanced app and desktop scaling"
                : parsedDpi <= 240
                  ? "Standard: tablet default sizing"
                  : "Large: touch-friendly oversized icons"}
          </p>
        </div>
      </div>

      <div role="group" aria-label="Size presets" className="flex flex-wrap gap-1.5 pt-1">
        {FLEX_SIZE_PRESETS.map((p) => {
          const selected = s.flexDisplaySize === p.val;
          return (
            <button
              key={p.val}
              type="button"
              aria-pressed={selected}
              onClick={() => setSize(p.val)}
              className={`btn-tactile min-h-8 cursor-pointer rounded px-2 text-[11px] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400 ${
                selected
                  ? "bg-gold-400 font-medium text-ink-950"
                  : "border border-ink-700 bg-ink-900 text-text hover:text-text-strong"
              }`}
            >
              {p.label}
            </button>
          );
        })}
      </div>

      <SwitchRow
        label="Hide Android taskbar"
        hint="Hides the navigation bar on the virtual display."
        checked={s.hideVirtualTaskbar ?? false}
        onChange={(v) => editMode("flex", { hideVirtualTaskbar: v })}
      />
    </Group>
  );
}

function CameraPanel() {
  const camera = useAppStore((st) => st.modeDefaults.camera);
  return (
    <div className="space-y-4">
      <Group title="Audio">
        <SwitchRow
          label="Use the phone microphone on this PC"
          hint="Off: no sound from the phone."
          checked={camera.playAudio}
          onChange={(v) => editMode("camera", { playAudio: v })}
        />
      </Group>
      <Group title="Quality">
        <SegmentedControl<BitrateSegment>
          label="Bitrate"
          hint="Starting choice in the Camera launch dialog. Higher is sharper but needs a strong connection."
          segments={BITRATE_SEGMENTS}
          value={camera.bitrate}
          customLabel={mbps(CAMERA_BITRATE_BPS[camera.bitrate])}
          onChange={(v) => editMode("camera", { bitrate: v })}
        />
      </Group>
      <Group title="Window options">
        <SwitchRow
          label="Show phone in one AnyLeap window"
          hint="Keeps the phone view and its toolbar together."
          checked={camera.embedded}
          onChange={(v) => editMode("camera", { embedded: v })}
        />
      </Group>
    </div>
  );
}

export function PhoneMirroringSection({
  targetMode,
}: {
  targetMode?: { mode: MirrorMode; tick: number } | null;
} = {}) {
  // Selecting a tab only changes the visible panel; it writes no settings (SPEC-28 §5.1).
  const [tab, setTab] = useState<MirrorMode>(targetMode?.mode ?? "mirror");
  const notice = useAppStore((s) => s.modeDefaultsNotice);
  const headingId = useId();

  useEffect(() => {
    if (targetMode) setTab(targetMode.mode);
  }, [targetMode?.tick, targetMode]);
  return (
    <Card aria-labelledby={headingId}>
      <CardHeader>
        <CardTitle id={headingId} className="sr-only">
          Phone mirroring
        </CardTitle>
        <p className="text-[11px] text-text-muted">
          Each mode keeps its own settings. Devices launches use the selected mode.
        </p>
      </CardHeader>
      <CardContent className="space-y-3">
        {notice && (
          <div
            role="status"
            className="flex items-start justify-between gap-3 rounded-lg border border-gold-400 bg-ink-950 p-3"
          >
            <p className="text-xs text-text-strong">
              Audio is now on in Mirror, Flex and High fidelity. Turn it off per mode here.
            </p>
            <button
              type="button"
              onClick={dismissModeNotice}
              className="btn-tactile min-h-8 shrink-0 cursor-pointer rounded border border-ink-700 bg-ink-900 px-2 text-xs text-text hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
            >
              Dismiss
            </button>
          </div>
        )}
        <ModeTabs active={tab} onSelect={setTab}>
          {tab === "camera" ? <CameraPanel /> : <DisplayModePanel key={tab} mode={tab} />}
        </ModeTabs>
      </CardContent>
    </Card>
  );
}
