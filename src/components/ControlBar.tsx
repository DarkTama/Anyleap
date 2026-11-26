import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  ArrowLeft,
  Bell,
  BellOff,
  Bluetooth,
  Camera,
  Circle,
  Disc,
  HelpCircle,
  Monitor,
  MonitorOff,
  Mouse,
  PhoneOff,
  Power,
  RotateCw,
  Square,
  Video,
  Volume1,
  Volume2,
  Wifi,
  X,
  Zap,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useAppStore } from "@/store/useAppStore";
import { logActivity } from "@/stores/activityStore";
import {
  openNotifications,
  sendKeyevent,
  toggleDeviceOrientation,
  sendCameraShortcut,
  restartWithScreenOff,
  listSessions,
  stopMirror,
  getPhoneToggles,
  setPhoneToggle,
  startRecording,
  stopRecording,
  endCall,
  getCallActive,
} from "@/lib/tauri";
import type { PhoneToggleKey, PhoneToggles, SessionMode } from "@/lib/types";
import type { DisplayMode } from "@/lib/modeSettings";
import { launchMode } from "@/lib/modeActions";
import { KEYCODE } from "@/lib/keycodes";
import type { ControlConfig } from "@/lib/controlConfig";
import { controlBarGroups, type ControlItem, type ControlSurface, type ControlItemId } from "@/lib/controlBarModel";
import { cn } from "@/lib/utils";
import { Tooltip, type TooltipSide } from "./Tooltip";
import { MorePopover, ICON_BTN, ICON_BTN_DANGER, ICON_BTN_PRESSED } from "./MorePopover";
import { ScrollPopover } from "./ScrollPopover";

const ICON = "h-5 w-5";
const MORE_ICON = "h-4 w-4";

export interface ControlBarNotice {
  text: string;
  actionLabel?: string;
  onAction?: () => void;
}

function iconFor(id: ControlItemId, screenOff: boolean, cls: string): ReactNode {
  switch (id) {
    case "back": return <ArrowLeft className={cls} />;
    case "home": return <Circle className={cls} />;
    case "recents": return <Square className={cls} />;
    case "volUp": return <Volume2 className={cls} />;
    case "volDown": return <Volume1 className={cls} />;
    case "power": return <Power className={cls} />;
    case "screenOff": return screenOff ? <Monitor className={cls} /> : <MonitorOff className={cls} />;
    case "orientToggle": return <RotateCw className={cls} />;
    case "screenshot": return <Camera className={cls} />;
    case "record": return <Disc className={cls} />;
    case "endCall": return <PhoneOff className={cls} />;
    case "notifications": return <Bell className={cls} />;
    case "wifi": return <Wifi className={cls} />;
    case "bluetooth": return <Bluetooth className={cls} />;
    case "dnd": return <BellOff className={cls} />;
    case "scroll": return <Mouse className={cls} />;
    case "torch": return <Zap className={cls} />;
    case "zoomIn": return <ZoomIn className={cls} />;
    case "zoomOut": return <ZoomOut className={cls} />;
    case "cameraHelp": return <HelpCircle className={cls} />;
    case "stopCamera": return <Power className={cls} />;
    default: return null;
  }
}

/** Android nav + media/power keys for a device (via adb input), grouped
 *  Navigate / Sound / Screen / Capture with a More popover (SPEC-28 phase 3).
 *  Which buttons show is driven by the user's control-bar config. */
export function ControlBar({
  serial,
  config,
  orientation,
  showOrientToggle = false,
  showScroll = true,
  sessionMode = "display",
  displayMode,
  surface = "embedded",
  onNotice,
  onOpenSettings,
}: {
  serial: string;
  config: ControlConfig;
  orientation: "horizontal" | "vertical";
  /** Rotate acts on the physical display only — hidden in flex display mode. */
  showOrientToggle?: boolean;
  /** Scroll quick-settings popover; hidden in Camera automatically. */
  showScroll?: boolean;
  sessionMode?: SessionMode;
  /** Active display mode preset ("mirror" | "flex" | "hifi") whose scroll settings are edited. */
  displayMode?: DisplayMode;
  /** `collapsed` only applies on the floating control window. */
  surface?: ControlSurface;
  /** Receives transient notices (e.g. recording saved). Falls back to a local toast. */
  onNotice?: (notice: ControlBarNotice) => void;
  /** Optional override for the Scroll popover "All settings" link. */
  onOpenSettings?: (mode: DisplayMode) => void;
}) {
  const setError = useAppStore((s) => s.setError);
  const sessions = useAppStore((s) => s.sessions);
  const sessionModes = useAppStore((s) => s.sessionModes);
  const currentMode = useAppStore((s) => s.currentMode);
  const upsertSession = useAppStore((s) => s.upsertSession);
  const deviceToggles = useAppStore((s) => s.deviceToggles);
  const setDeviceToggle = useAppStore((s) => s.setDeviceToggle);
  const toggles = deviceToggles[serial];
  const screenOff = !!toggles?.screenOff;
  const [torchOn, setTorchOn] = useState(false);
  const [showGuide, setShowGuide] = useState(false);
  const [phoneToggles, setPhoneToggles] = useState<PhoneToggles | null>(null);
  const [recBusy, setRecBusy] = useState(false);
  const [inCall, setInCall] = useState(false);
  const [localNotice, setLocalNotice] = useState<ControlBarNotice | null>(null);

  const recordingSession = sessions.find((s) => s.serial === serial && !!s.recording_path);
  const activeSession = sessions.find((s) => s.serial === serial && s.mode !== "camera");
  const isRecording = !!recordingSession;
  const recordingPath = recordingSession?.recording_path ?? null;
  const camera = sessionMode === "camera";
  const resolvedSessionMode = activeSession ? sessionModes[activeSession.id] : undefined;
  const activeDisplayMode: DisplayMode =
    displayMode ??
    (resolvedSessionMode && resolvedSessionMode !== "camera"
      ? resolvedSessionMode
      : launchMode(currentMode));

  useEffect(() => {
    let cancelled = false;
    getPhoneToggles(serial)
      .then((res) => {
        if (!cancelled) setPhoneToggles(res);
      })
      .catch(() => {
        // best-effort when device is offline/unauthorized
      });
    return () => {
      cancelled = true;
    };
  }, [serial]);

  // End call is only offered during a call: poll the call state while enabled.
  useEffect(() => {
    if (camera || !config.buttons.endCall) {
      setInCall(false);
      return;
    }
    let cancelled = false;
    const poll = () => {
      if (document.hidden) return;
      getCallActive(serial)
        .then((active) => {
          if (!cancelled) setInCall(active);
        })
        .catch(() => {
          if (!cancelled) setInCall(false);
        });
    };
    poll();
    const id = setInterval(poll, 4000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [serial, camera, config.buttons.endCall]);

  useEffect(() => {
    if (!localNotice) return;
    const id = setTimeout(() => setLocalNotice(null), 8000);
    return () => clearTimeout(id);
  }, [localNotice]);

  const notify = (n: ControlBarNotice) => (onNotice ? onNotice(n) : setLocalNotice(n));
  const err = (e: unknown) => setError(String(e));
  const key = (code: number) => sendKeyevent(serial, code).catch(err);

  const toggleRecord = () => {
    if (recBusy) return;
    setRecBusy(true);
    const wasRecording = isRecording;
    const savedPath = recordingPath;
    const op = wasRecording ? stopRecording(serial) : startRecording(serial);
    op.then((next) => {
      upsertSession(next);
      if (wasRecording) {
        notify({
          text: "Recording saved",
          actionLabel: savedPath ? "Show in folder" : undefined,
          onAction: savedPath ? () => revealItemInDir(savedPath).catch(err) : undefined,
        });
      }
    })
      .catch(err)
      .finally(() => setRecBusy(false));
  };

  const handlePhoneToggle = (k: PhoneToggleKey) => {
    const next = !(phoneToggles?.[k] ?? false);
    setPhoneToggle(serial, k, next)
      .then(() =>
        setPhoneToggles((prev) => ({
          wifi: prev?.wifi ?? false,
          bluetooth: prev?.bluetooth ?? false,
          dnd: prev?.dnd ?? false,
          [k]: next,
        })),
      )
      .catch(err);
  };

  const toggleScreenOff = () => {
    const next = !screenOff;
    if (camera) {
      sendKeyevent(serial, KEYCODE.POWER)
        .then(() => setDeviceToggle(serial, "screenOff", next))
        .catch(err);
    } else {
      restartWithScreenOff(serial, next)
        .then(() => setDeviceToggle(serial, "screenOff", next))
        .catch(() => {
          sendKeyevent(serial, next ? KEYCODE.SLEEP : KEYCODE.WAKEUP)
            .then(() => setDeviceToggle(serial, "screenOff", next))
            .catch(err);
        });
    }
  };
  const rotate = () => {
    if (camera) sendCameraShortcut(serial, "rotate_cw").catch(err);
    else toggleDeviceOrientation(serial).catch(err);
  };
  const toggleTorch = () => {
    const next = !torchOn;
    sendCameraShortcut(serial, next ? "torch_on" : "torch_off")
      .then(() => setTorchOn(next))
      .catch(err);
  };
  const stopCameraStream = async () => {
    const list = await listSessions().catch(() => []);
    const current = list.find((s) => s.serial === serial);
    if (current) stopMirror(current.id).catch(err);
  };

  const activate = (item: ControlItem) => {
    switch (item.id) {
      case "back": void key(KEYCODE.BACK); break;
      case "home": void key(KEYCODE.HOME); break;
      case "recents": void key(KEYCODE.RECENTS); break;
      case "volUp": void key(KEYCODE.VOLUME_UP); break;
      case "volDown": void key(KEYCODE.VOLUME_DOWN); break;
      case "power": void key(KEYCODE.POWER); break;
      case "screenOff": toggleScreenOff(); break;
      case "orientToggle": rotate(); break;
      case "screenshot":
        void key(KEYCODE.SCREENSHOT);
        logActivity({ kind: "screenshot", title: "Screenshot taken", detail: serial });
        break;
      case "record": toggleRecord(); break;
      case "endCall": endCall(serial).catch(err); break;
      case "notifications": openNotifications(serial).catch(err); break;
      case "wifi": handlePhoneToggle("wifi"); break;
      case "bluetooth": handlePhoneToggle("bluetooth"); break;
      case "dnd": handlePhoneToggle("dnd"); break;
      case "torch": toggleTorch(); break;
      case "zoomIn": sendCameraShortcut(serial, "zoom_in").catch(err); break;
      case "zoomOut": sendCameraShortcut(serial, "zoom_out").catch(err); break;
      case "cameraHelp": setShowGuide((v) => !v); break;
      case "stopCamera": void stopCameraStream(); break;
    }
  };

  const model = useMemo(
    () =>
      controlBarGroups(config, sessionMode, {
        inCall,
        isRecording,
        screenOff,
        torchOn,
        wifi: !!phoneToggles?.wifi,
        bluetooth: !!phoneToggles?.bluetooth,
        dnd: !!phoneToggles?.dnd,
        showOrient: showOrientToggle,
        showScroll,
      }, surface),
    [config, sessionMode, surface, inCall, isRecording, screenOff, torchOn, phoneToggles, showOrientToggle, showScroll],
  );

  const vertical = orientation === "vertical";
  const tipSide: TooltipSide = vertical ? "left" : "bottom";
  const popoverSide: TooltipSide = vertical ? "left" : "right";

  const moreIcons: Partial<Record<string, ReactNode>> = {};
  for (const item of model.more) moreIcons[item.id] = iconFor(item.id, screenOff, MORE_ICON);

  const renderItem = (item: ControlItem) => {
    if (item.id === "scroll") {
      return (
        <ScrollPopover
          key={item.id}
          serial={serial}
          mode={activeDisplayMode}
          side={popoverSide}
          onOpenSettings={onOpenSettings}
        />
      );
    }
    const toggle = item.kind === "toggle";
    return (
      <Tooltip key={item.id} label={item.label} hint={item.hint} side={tipSide}>
        <button
          type="button"
          aria-label={item.label}
          aria-pressed={toggle ? !!item.pressed : undefined}
          disabled={item.id === "record" && recBusy}
          className={cn(
            ICON_BTN,
            item.danger && ICON_BTN_DANGER,
            item.pressed && !item.danger && ICON_BTN_PRESSED,
          )}
          onClick={() => activate(item)}
        >
          <span className="relative inline-flex items-center justify-center">
            {iconFor(item.id, screenOff, ICON)}
            {item.id === "record" && item.pressed && (
              <span className="absolute -right-1 -top-1 h-2 w-2 rounded-full bg-status-danger" />
            )}
          </span>
        </button>
      </Tooltip>
    );
  };

  const container = vertical ? "flex h-full min-h-min flex-col items-center p-2.5" : "border-t border-ink-700 px-3 py-3";
  const wrap = vertical ? "flex w-full flex-1 flex-col items-center gap-1.5" : "flex flex-wrap items-center gap-2";
  const divider = vertical ? (
    <div role="separator" className="my-0.5 h-px w-8 bg-ink-700" />
  ) : (
    <div role="separator" aria-orientation="vertical" className="mx-1 h-6 w-px bg-ink-700" />
  );

  return (
    <div className={container} role="toolbar" aria-label="Phone controls" aria-orientation={orientation}>
      <div className={wrap}>
        {model.groups.map((g, i) => (
          <div
            key={g.id}
            role="group"
            aria-label={g.label}
            className={vertical ? "flex flex-col items-center gap-1.5" : "flex items-center gap-2"}
          >
            {i > 0 && divider}
            {g.items.map(renderItem)}
          </div>
        ))}
        {model.more.length > 0 && (
          <div className={vertical ? "mt-auto flex flex-col items-center gap-1.5 pt-1.5" : "flex items-center gap-2"}>
            {!vertical && divider}
            <MorePopover
              items={model.more}
              icons={moreIcons}
              onActivate={activate}
              side={vertical ? "left" : "right"}
            />
          </div>
        )}
      </div>
      {localNotice && (
        <div
          role="status"
          className="popover-panel fixed bottom-4 left-1/2 z-50 flex -translate-x-1/2 items-center gap-3 rounded-lg border border-gold-400/50 bg-ink-800 px-3 py-1.5 text-xs text-gold-400 shadow-elevated"
        >
          <span>{localNotice.text}</span>
          {localNotice.onAction && (
            <button
              type="button"
              onClick={localNotice.onAction}
              className="btn-tactile cursor-pointer rounded border border-gold-400/60 px-2 py-0.5 font-semibold text-gold-400 hover:bg-ink-700"
            >
              {localNotice.actionLabel}
            </button>
          )}
        </div>
      )}
      {showGuide && (
        <div className="modal-backdrop fixed inset-0 z-50 flex items-center justify-center bg-ink-950/80 p-4">
          <div className="modal-panel relative w-full max-w-sm space-y-3 rounded-xl border border-ink-700 bg-ink-900 p-4 text-left shadow-elevated">
            <div className="flex items-center justify-between border-b border-ink-700 pb-2">
              <div className="flex items-center gap-2 text-xs font-bold text-mist-300">
                <Video className="h-4 w-4" />
                <span>Use AnyLeap Camera in any app</span>
              </div>
              <button
                type="button"
                aria-label="Close"
                onClick={() => setShowGuide(false)}
                className="btn-tactile rounded p-1 text-text-muted hover:bg-ink-800 hover:text-text-strong"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            </div>
            <ol className="list-inside list-decimal space-y-2 text-[11px] leading-relaxed text-text">
              <li>
                In your video-call or streaming app, add a <strong className="text-text-strong">window capture</strong>{" "}
                source (or share this window).
              </li>
              <li>
                Select <strong className="text-gold-400">AnyLeap Camera Studio</strong>.
              </li>
              <li>
                If the app offers a virtual camera, start it and choose it as your camera in the call settings.
              </li>
            </ol>
            <button
              type="button"
              onClick={() => setShowGuide(false)}
              className="btn-tactile w-full cursor-pointer rounded-lg bg-gold-400 py-1.5 text-xs font-semibold text-ink-950 hover:bg-gold-600"
            >
              Got it
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
