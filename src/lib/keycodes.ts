// Android key codes used by the mirror control bar (adb shell input keyevent).
export const KEYCODE = {
  BACK: 4,
  HOME: 3,
  RECENTS: 187, // APP_SWITCH
  VOLUME_UP: 24,
  VOLUME_DOWN: 25,
  POWER: 26,
  SCREENSHOT: 120, // KEYCODE_SYSRQ
} as const;
