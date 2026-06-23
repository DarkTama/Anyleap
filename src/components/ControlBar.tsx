import { useState } from "react";
import {
  ArrowLeft,
  Bell,
  Camera,
  Circle,
  Monitor,
  MonitorOff,
  Moon,
  Power,
  Square,
  Sun,
  Volume2,
  VolumeX,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/useAppStore";
import { openNotifications, restartWithScreenOff, sendKeyevent } from "@/lib/tauri";
import { KEYCODE } from "@/lib/keycodes";

const btn = "h-14 w-16 flex-col gap-1 px-1 text-[10px]";

/** Android navigation + media/power keys for a connected device (via adb input).
 *  Horizontal by default (inline in the Devices tab); vertical for the floating
 *  control window. */
export function ControlBar({
  serial,
  orientation = "horizontal",
}: {
  serial: string;
  orientation?: "horizontal" | "vertical";
}) {
  const setError = useAppStore((s) => s.setError);
  const [asleep, setAsleep] = useState(false);
  const [screenOff, setScreenOff] = useState(false);

  const key = (code: number) => () =>
    sendKeyevent(serial, code).catch((e) => setError(String(e)));
  const notif = () => openNotifications(serial).catch((e) => setError(String(e)));

  // Instant adb sleep/wake (phone really sleeps; mirror shows lock screen).
  const toggleSleep = () => {
    const next = !asleep;
    sendKeyevent(serial, next ? KEYCODE.SLEEP : KEYCODE.WAKEUP)
      .then(() => setAsleep(next))
      .catch((e) => setError(String(e)));
  };
  // True scrcpy screen-off: restart the mirror with --turn-screen-off toggled.
  const toggleScreenOff = () => {
    const next = !screenOff;
    restartWithScreenOff(serial, next)
      .then(() => setScreenOff(next))
      .catch((e) => setError(String(e)));
  };

  const container =
    orientation === "vertical"
      ? "p-2"
      : "border-t border-zinc-100 px-3 py-3 dark:border-zinc-800/60";
  const wrap = orientation === "vertical" ? "flex flex-col gap-2" : "flex flex-wrap gap-2";

  return (
    <div className={container}>
      <div className={wrap}>
        <Button variant="outline" className={btn} onClick={key(KEYCODE.BACK)}>
          <ArrowLeft className="h-4 w-4" />
          Back
        </Button>
        <Button variant="outline" className={btn} onClick={key(KEYCODE.HOME)}>
          <Circle className="h-4 w-4" />
          Home
        </Button>
        <Button variant="outline" className={btn} onClick={key(KEYCODE.RECENTS)}>
          <Square className="h-4 w-4" />
          Recents
        </Button>
        <Button variant="outline" className={btn} onClick={key(KEYCODE.VOLUME_UP)}>
          <Volume2 className="h-4 w-4" />
          Vol +
        </Button>
        <Button variant="outline" className={btn} onClick={key(KEYCODE.VOLUME_DOWN)}>
          <VolumeX className="h-4 w-4" />
          Vol −
        </Button>
        <Button variant="outline" className={btn} onClick={key(KEYCODE.POWER)}>
          <Power className="h-4 w-4" />
          Power
        </Button>
        <Button variant="outline" className={btn} onClick={key(KEYCODE.SCREENSHOT)}>
          <Camera className="h-4 w-4" />
          Shot
        </Button>
        <Button variant="outline" className={btn} onClick={notif}>
          <Bell className="h-4 w-4" />
          Notif
        </Button>
        <Button variant="outline" className={btn} onClick={toggleSleep}>
          {asleep ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          {asleep ? "Wake" : "Sleep"}
        </Button>
        <Button variant="outline" className={btn} onClick={toggleScreenOff}>
          {screenOff ? <Monitor className="h-4 w-4" /> : <MonitorOff className="h-4 w-4" />}
          {screenOff ? "Scr on" : "Scr off"}
        </Button>
      </div>
    </div>
  );
}
