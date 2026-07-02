import { useState } from "react";
import {
  ArrowLeft,
  Bell,
  Camera,
  Circle,
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
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/useAppStore";
import {
  openNotifications,
  restartWithScreenOff,
  sendKeyevent,
  setWheelSwipe,
  toggleDeviceOrientation,
} from "@/lib/tauri";
import { KEYCODE } from "@/lib/keycodes";
import type { ControlConfig, ControlSize } from "@/lib/controlConfig";

const SIZE: Record<ControlSize, { btn: string; icon: string }> = {
  sm: { btn: "h-12 w-14 flex-col gap-0.5 px-1 text-[9px]", icon: "h-3.5 w-3.5" },
  md: { btn: "h-14 w-16 flex-col gap-1 px-1 text-[10px]", icon: "h-4 w-4" },
  lg: { btn: "h-16 w-20 flex-col gap-1 px-1 text-xs", icon: "h-5 w-5" },
};

/** Android nav + media/power keys for a device (via adb input). Buttons shown,
 *  size, and orientation are driven by the user's control-bar config. */
export function ControlBar({
  serial,
  config,
  orientation,
  showOrientToggle = false,
  showSwipeScroll = false,
}: {
  serial: string;
  config: ControlConfig;
  orientation: "horizontal" | "vertical";
  /** Rotate acts on the physical display only — hidden in flex display mode. */
  showOrientToggle?: boolean;
  /** Wheel→swipe needs a mirror window; only the floating control strip shows it. */
  showSwipeScroll?: boolean;
}) {
  const setError = useAppStore((s) => s.setError);
  const [asleep, setAsleep] = useState(false);
  const [screenOff, setScreenOff] = useState(false);
  const [swipeScroll, setSwipeScroll] = useState(false);
  const sz = SIZE[config.size];
  const b = config.buttons;

  const key = (code: number) => () =>
    sendKeyevent(serial, code).catch((e) => setError(String(e)));
  const notif = () => openNotifications(serial).catch((e) => setError(String(e)));
  const toggleSleep = () => {
    const next = !asleep;
    sendKeyevent(serial, next ? KEYCODE.SLEEP : KEYCODE.WAKEUP)
      .then(() => setAsleep(next))
      .catch((e) => setError(String(e)));
  };
  const toggleScreenOff = () => {
    const next = !screenOff;
    restartWithScreenOff(serial, next)
      .then(() => setScreenOff(next))
      .catch((e) => setError(String(e)));
  };
  const rotate = () => toggleDeviceOrientation(serial).catch((e) => setError(String(e)));
  const toggleSwipeScroll = () => {
    const next = !swipeScroll;
    setWheelSwipe(serial, next)
      .then(() => setSwipeScroll(next))
      .catch((e) => setError(String(e)));
  };

  const container =
    orientation === "vertical"
      ? "p-2"
      : "border-t border-zinc-100 px-3 py-3 dark:border-zinc-800/60";
  const wrap =
    orientation === "vertical"
      ? "flex flex-col items-center gap-2"
      : "flex flex-wrap gap-2";

  return (
    <div className={container}>
      <div className={wrap}>
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
      </div>
    </div>
  );
}
