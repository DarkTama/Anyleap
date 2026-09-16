import { useState } from "react";
import {
  ArrowLeft,
  Bell,
  Camera,
  Circle,
  HelpCircle,
  Monitor,
  MonitorOff,
  Moon,
  Mouse,
  Power,
  RotateCw,
  Square,
  Sun,
  Volume2,
  VolumeX,
  X,
  Zap,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/useAppStore";
import {
  openNotifications,
  sendKeyevent,
  setWheelSwipe,
  toggleDeviceOrientation,
  sendCameraShortcut,
  restartWithScreenOff,
  listSessions,
  stopMirror,
} from "@/lib/tauri";
import type { SessionMode } from "@/lib/types";
import { KEYCODE } from "@/lib/keycodes";
import type { ControlConfig, ControlSize } from "@/lib/controlConfig";

const SIZE: Record<ControlSize, { btn: string; icon: string }> = {
  sm: { btn: "h-8 w-8 flex-col gap-0.5 p-0 text-[9px] hover:bg-zinc-800 hover:text-white text-zinc-300 border-zinc-800/60", icon: "h-3.5 w-3.5" },
  md: { btn: "h-9 w-9 flex-col gap-0.5 p-0 text-[10px] hover:bg-zinc-800 hover:text-white text-zinc-300 border-zinc-800/60", icon: "h-4 w-4" },
  lg: { btn: "h-11 w-11 flex-col gap-1 p-0 text-xs hover:bg-zinc-800 hover:text-white text-zinc-300 border-zinc-800/60", icon: "h-5 w-5" },
};

/** Android nav + media/power keys for a device (via adb input). Buttons shown,
 *  size, and orientation are driven by the user's control-bar config. */
export function ControlBar({
  serial,
  config,
  orientation,
  showOrientToggle = false,
  showSwipeScroll = false,
  sessionMode = "display",
}: {
  serial: string;
  config: ControlConfig;
  orientation: "horizontal" | "vertical";
  /** Rotate acts on the physical display only — hidden in flex display mode. */
  showOrientToggle?: boolean;
  /** Wheel→swipe needs a mirror window; only the floating control strip shows it. */
  showSwipeScroll?: boolean;
  sessionMode?: SessionMode;
}) {
  const setError = useAppStore((s) => s.setError);
  const deviceToggles = useAppStore((s) => s.deviceToggles);
  const setDeviceToggle = useAppStore((s) => s.setDeviceToggle);
  const toggles = deviceToggles[serial];
  const asleep = !!toggles?.asleep;
  const screenOff = !!toggles?.screenOff;
  const swipeScroll = !!toggles?.swipeScroll;
  const sz = SIZE[config.size];
  const [torchOn, setTorchOn] = useState(false);
  const [showObsGuide, setShowObsGuide] = useState(false);
  const toggleTorch = () => {
    const next = !torchOn;
    sendCameraShortcut(serial, next ? "torch_on" : "torch_off")
      .then(() => setTorchOn(next))
      .catch((e) => setError(String(e)));
  };
  const zoomIn = () => sendCameraShortcut(serial, "zoom_in").catch((e) => setError(String(e)));
  const zoomOut = () => sendCameraShortcut(serial, "zoom_out").catch((e) => setError(String(e)));

  const b = config.buttons;

  const key = (code: number) => () =>
    sendKeyevent(serial, code).catch((e) => setError(String(e)));
  const notif = () => openNotifications(serial).catch((e) => setError(String(e)));
  const toggleSleep = () => {
    const next = !asleep;
    sendKeyevent(serial, next ? KEYCODE.SLEEP : KEYCODE.WAKEUP)
      .then(() => setDeviceToggle(serial, "asleep", next))
      .catch((e) => setError(String(e)));
  };
  const toggleScreenOff = () => {
    const next = !screenOff;
    if (sessionMode === "camera") {
      sendKeyevent(serial, KEYCODE.POWER)
        .then(() => setDeviceToggle(serial, "screenOff", next))
        .catch((e) => setError(String(e)));
    } else {
      restartWithScreenOff(serial, next)
        .then(() => setDeviceToggle(serial, "screenOff", next))
        .catch(() => {
          sendKeyevent(serial, next ? KEYCODE.SLEEP : KEYCODE.WAKEUP)
            .then(() => setDeviceToggle(serial, "screenOff", next))
            .catch((e) => setError(String(e)));
        });
    }
  };
  const rotate = () => {
    if (sessionMode === "camera") {
      sendCameraShortcut(serial, "rotate_cw").catch((e) => setError(String(e)));
    } else {
      toggleDeviceOrientation(serial).catch((e) => setError(String(e)));
    }
  };
  const toggleSwipeScroll = () => {
    const next = !swipeScroll;
    setWheelSwipe(serial, next)
      .then(() => setDeviceToggle(serial, "swipeScroll", next))
      .catch((e) => setError(String(e)));
  };

  const container =
    orientation === "vertical"
      ? "p-1.5"
      : "border-t border-zinc-100 px-3 py-3 dark:border-zinc-800/60";
  const wrap =
    orientation === "vertical"
      ? "flex flex-col items-center gap-1.5"
      : "flex flex-wrap gap-2";
  return (
    <div className={container}>
      <div className={wrap}>
        {sessionMode === "camera" ? (
          <>
            <Button
              variant={torchOn ? "default" : "outline"}
              className={`${sz.btn} ${torchOn ? "bg-amber-600 hover:bg-amber-500 text-white" : ""}`}
              onClick={toggleTorch}
              title="Toggle Camera Flash / Torch"
            >
              <Zap className={sz.icon} />
              Torch
            </Button>
            <Button
              variant="outline"
              className={sz.btn}
              onClick={zoomIn}
              title="Zoom In (Alt+Up)"
            >
              <ZoomIn className={sz.icon} />
              Zoom+
            </Button>
            <Button
              variant="outline"
              className={sz.btn}
              onClick={zoomOut}
              title="Zoom Out (Alt+Down)"
            >
              <ZoomOut className={sz.icon} />
              Zoom−
            </Button>
            {b.screenOff && (
              <Button variant="outline" className={sz.btn} onClick={toggleScreenOff}>
                {screenOff ? <Monitor className={sz.icon} /> : <MonitorOff className={sz.icon} />}
                {screenOff ? "Scr on" : "Scr off"}
              </Button>
            )}
            {b.orientToggle && showOrientToggle && (
              <Button variant="outline" className={sz.btn} onClick={rotate}>
                <RotateCw className={sz.icon} />
                Rotate
              </Button>
            )}
            <Button
              variant="outline"
              className={`${sz.btn} text-sky-400 hover:bg-sky-950/50 hover:text-sky-300 border-sky-900/40`}
              onClick={() => setShowObsGuide((v) => !v)}
              title="How to use with Google Meet, Zoom, or OBS"
            >
              <HelpCircle className={sz.icon} />
              Guide
            </Button>
            <Button
              variant="outline"
              onClick={async () => {
                const sessions = await listSessions().catch(() => []);
                const current = sessions.find((s) => s.serial === serial);
                if (current) {
                  stopMirror(current.id).catch((e) => setError(String(e)));
                }
              }}
              title="Stop Camera Stream"
            >
              <Power className={sz.icon} />
              Stop
            </Button>
          </>
        ) : (
          <>
        {b.back && (
          <Button variant="outline" className={sz.btn} onClick={key(KEYCODE.BACK)}>
            <ArrowLeft className={sz.icon} />
            Back
          </Button>
        )}
        {b.home && (
          <Button variant="outline" className={sz.btn} onClick={key(KEYCODE.HOME)}>
            <Circle className={sz.icon} />
            Home
          </Button>
        )}
        {b.recents && (
          <Button variant="outline" className={sz.btn} onClick={key(KEYCODE.RECENTS)}>
            <Square className={sz.icon} />
            Recents
          </Button>
        )}
        {b.volUp && (
          <Button variant="outline" className={sz.btn} onClick={key(KEYCODE.VOLUME_UP)}>
            <Volume2 className={sz.icon} />
            Vol +
          </Button>
        )}
        {b.volDown && (
          <Button variant="outline" className={sz.btn} onClick={key(KEYCODE.VOLUME_DOWN)}>
            <VolumeX className={sz.icon} />
            Vol −
          </Button>
        )}
        {b.power && (
          <Button variant="outline" className={sz.btn} onClick={key(KEYCODE.POWER)}>
            <Power className={sz.icon} />
            Power
          </Button>
        )}
        {b.screenshot && (
          <Button variant="outline" className={sz.btn} onClick={key(KEYCODE.SCREENSHOT)}>
            <Camera className={sz.icon} />
            Shot
          </Button>
        )}
        {b.notifications && (
          <Button variant="outline" className={sz.btn} onClick={notif}>
            <Bell className={sz.icon} />
            Notif
          </Button>
        )}
        {b.sleep && (
          <Button variant="outline" className={sz.btn} onClick={toggleSleep}>
            {asleep ? <Sun className={sz.icon} /> : <Moon className={sz.icon} />}
            {asleep ? "Wake" : "Sleep"}
          </Button>
        )}
        {b.screenOff && (
          <Button variant="outline" className={sz.btn} onClick={toggleScreenOff}>
            {screenOff ? <Monitor className={sz.icon} /> : <MonitorOff className={sz.icon} />}
            {screenOff ? "Scr on" : "Scr off"}
          </Button>
        )}
        {b.orientToggle && showOrientToggle && (
          <Button variant="outline" className={sz.btn} onClick={rotate}>
            <RotateCw className={sz.icon} />
            Rotate
          </Button>
        )}
        {b.swipeScroll && showSwipeScroll && (
          <Button
            variant={swipeScroll ? "default" : "outline"}
            className={sz.btn}
            onClick={toggleSwipeScroll}
            title="Wheel scrolls as touch flings — for Reels/Shorts"
          >
            <Mouse className={sz.icon} />
            {swipeScroll ? "Swipe on" : "Swipe off"}
          </Button>
        )}
          </>
        )}
      </div>
      {showObsGuide && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-xs">
          <div className="relative w-full max-w-sm rounded-xl border border-sky-500/30 bg-[#0e121a] p-4 text-left shadow-2xl space-y-3">
            <div className="flex items-center justify-between border-b border-zinc-800 pb-2">
              <div className="flex items-center gap-2 text-xs font-bold text-sky-400">
                <Camera className="h-4 w-4" />
                <span>Google Meet & OBS Virtual Camera</span>
              </div>
              <button
                type="button"
                onClick={() => setShowObsGuide(false)}
                className="rounded p-1 text-zinc-400 hover:bg-zinc-800 hover:text-white"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            </div>
            <ol className="list-decimal list-inside space-y-2 text-[11px] text-zinc-300 leading-relaxed">
              <li>
                Open <strong className="text-white">OBS Studio</strong> (free desktop app).
              </li>
              <li>
                In Sources, click <strong>+ → Window Capture</strong>.
              </li>
              <li>
                Select <strong className="text-cyan-300">AnyLeap Camera Studio</strong>.
              </li>
              <li>
                In OBS (bottom right), click <strong className="text-emerald-400">Start Virtual Camera</strong>.
              </li>
              <li>
                In <strong className="text-sky-300">meet.google.com</strong> or Zoom settings, choose <strong className="text-sky-300">OBS Virtual Camera</strong>.
              </li>
            </ol>
            <button
              type="button"
              onClick={() => setShowObsGuide(false)}
              className="w-full rounded-lg bg-sky-600 py-1.5 text-xs font-semibold text-white hover:bg-sky-500 transition-colors cursor-pointer"
            >
              Got it
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
