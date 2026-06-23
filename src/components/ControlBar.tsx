import { ArrowLeft, Bell, Camera, Circle, Power, Square, Volume2, VolumeX } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/useAppStore";
import { openNotifications, sendKeyevent } from "@/lib/tauri";
import { KEYCODE } from "@/lib/keycodes";

const btn = "h-14 w-16 flex-col gap-1 px-1 text-[10px]";

/** Android navigation + media/power keys for a connected device (via adb input). */
export function ControlBar({ serial }: { serial: string }) {
  const setError = useAppStore((s) => s.setError);
  const key = (code: number) => () =>
    sendKeyevent(serial, code).catch((e) => setError(String(e)));
  const notif = () => openNotifications(serial).catch((e) => setError(String(e)));

  return (
    <div className="border-t border-zinc-100 px-3 py-3 dark:border-zinc-800/60">
      <div className="flex flex-wrap gap-2">
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
      </div>
    </div>
  );
}
