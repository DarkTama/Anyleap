/**
 * Settings registry (SPEC-28 §7, §8): one entry per control with its section, label,
 * helper text and search keywords. Section components read copy from here so the
 * filter and the UI cannot drift apart.
 */
export type SectionId =
  | "general"
  | "phone"
  | "controlbar"
  | "mousekb"
  | "clipboard"
  | "audio"
  | "advanced"
  | "about";

export const SECTIONS: { id: SectionId; label: string }[] = [
  { id: "general", label: "General" },
  { id: "phone", label: "Phone mirroring" },
  { id: "controlbar", label: "Control bar" },
  { id: "mousekb", label: "Shared mouse & keyboard" },
  { id: "clipboard", label: "Clipboard & files" },
  { id: "audio", label: "Audio, media & power" },
  { id: "advanced", label: "Advanced" },
  { id: "about", label: "About" },
];

export interface SettingEntry {
  id: string;
  section: SectionId;
  label: string;
  helper?: string;
  keywords?: string[];
}

const e = (
  id: string,
  section: SectionId,
  label: string,
  helper?: string,
  keywords: string[] = [],
): SettingEntry => ({ id, section, label, helper, keywords });

export const REGISTRY: SettingEntry[] = [
  // General
  e("M-25", "general", "Minimize to tray when closing the window", undefined, ["tray", "close", "background"]),
  e("M-26", "general", "Check for updates on launch", undefined, ["update", "version"]),
  e("M-27", "general", "Show phone notifications on this PC", undefined, ["notification", "popup", "toast"]),
  e("M-28", "general", "Battery alerts", "Alerts at 20% and when full.", ["battery", "charge", "low"]),
  e("M-29", "general", "Updates", "Check for a newer version of AnyLeap.", ["update", "version", "release"]),

  // Phone mirroring (rendered as one block; any match shows the whole section)
  e("M-01", "phone", "Quality", "How good is your connection? Smooth saves bandwidth; Maximum needs USB or strong Wi-Fi.", ["preset", "smooth", "balanced", "sharp", "maximum"]),
  e("M-02", "phone", "Resolution", "Long side of the video. Native is sharpest.", ["size", "1080p", "720p", "native"]),
  e("M-03", "phone", "Bitrate", "Higher is sharper but needs a strong connection.", ["mbps", "bandwidth"]),
  e("M-04", "phone", "Frame rate", "60 is smoother; 30 saves bandwidth.", ["fps", "smooth"]),
  e("M-05", "phone", "Codec", "H.265 saves bandwidth. Needs a phone that supports it.", ["h264", "h265", "av1", "video"]),
  e("M-06", "phone", "Keep phone awake", "Stops the phone sleeping while mirroring.", ["sleep", "stay awake"]),
  e("M-07", "phone", "Turn phone screen off", "Saves battery. Mirroring keeps working.", ["screen", "display", "battery"]),
  e("M-08", "phone", "Start in fullscreen", undefined, ["full screen", "window"]),
  e("M-09", "phone", "Show touches on the phone", "Draws a dot where a finger or click lands.", ["tap", "finger"]),
  e("M-10", "phone", "Play phone audio on this PC", "Off: sound stays on the phone.", ["sound", "mute", "audio", "microphone"]),
  e("M-11", "phone", "Allow control from this PC", "Off: watch only.", ["view only", "input", "mouse"]),
  e("M-12", "phone", "Use Android on-screen keyboard", "Off: type with this PC's keyboard directly.", ["ime", "typing"]),
  e("M-14", "phone", "Show phone in one AnyLeap window", "Keeps the phone view and its toolbar together.", ["embedded", "sidebar", "integrated"]),
  e("M-15", "phone", "Virtual display size", "Match this PC's monitor for a desktop-like layout. Format 1920x1080/240 (pixels / dpi). Empty means automatic.", ["flex", "dpi", "desktop mode"]),
  e("M-16", "phone", "Use this PC's resolution", "Detects this PC's main monitor.", ["flex", "monitor"]),
  e("M-18", "phone", "Size presets", undefined, ["flex", "compact", "hidpi"]),
  e("M-19", "phone", "Hide Android taskbar", "Hides the navigation bar on the virtual display.", ["flex", "navigation bar"]),
  e("M-20", "phone", "Keep phone aspect ratio", "Off: the window can be resized to any shape.", ["resize", "lock"]),
  e("M-20b", "phone", "Picture fit", "Letterbox keeps proportions. Stretched fills the window.", ["letterbox", "unscaled", "stretched"]),
  e("M-21", "phone", "Scroll style", "Smooth scrolls like a normal page. Paged turns each wheel notch into a swipe, for short-video feeds.", ["scroll", "wheel", "smooth", "paged", "reading", "short videos"]),
  e("M-22", "phone", "Scroll speed", "Matches the phone's normal feel.", ["scroll", "speed", "multiplier", "auto", "density"]),
  e("M-23", "phone", "Smooth scrolling", "Glides between wheel notches. Adds a little delay, so turn it off for games.", ["scroll", "smooth", "glide", "ease", "wheel"]),
  e("M-24", "phone", "Touchpad speed", "Touchpads already scroll precisely, so this has its own speed.", ["scroll", "touchpad", "trackpad", "speed", "multiplier"]),

  // Control bar
  e("C-01", "controlbar", "Dock side", undefined, ["position", "left", "right", "undocked"]),
  e("C-02", "controlbar", "Button size", undefined, ["small", "medium", "large"]),
  e("C-03", "controlbar", "Buttons", "Choose which buttons appear.", ["toolbar", "strip"]),

  // Shared mouse & keyboard
  e("K-01", "mousekb", "Crossing delay", "How long the cursor must push at the edge before it crosses. Edit it in Computers.", ["dwell", "edge", "resistance"]),
  e("K-02", "mousekb", "Corner dead zone", "Top and bottom corner zones where crossing is blocked (tabs, title bars, taskbars). Edit it in Computers.", ["deadzone", "corner", "margin"]),
  e("K-03", "mousekb", "Show which PC has the cursor", "A small floating badge.", ["indicator", "badge", "overlay"]),
  e("K-04", "mousekb", "Badge position", undefined, ["corner", "top", "bottom", "left", "right"]),
  e("K-05", "mousekb", "Show cursor position in the layout map", "Marks the live cursor on the Computers layout.", ["dot", "canvas", "topology"]),
  e("K-11", "mousekb", "Emergency unhook shortcut", "Immediately releases the cursor lock and returns the pointer to this PC.", ["escape", "scroll lock", "pause"]),

  // Clipboard & files
  e("K-06", "clipboard", "Share copied text", "Text copied on one PC can be pasted on the other.", ["clipboard", "sync"]),
  e("K-07", "clipboard", "Share copied images", "Includes screenshots.", ["clipboard", "bitmap", "screenshot", "sync"]),
  e("T-01", "clipboard", "Drag and drop files between PCs (Experimental)", "Drag files across the edge. Some apps reject drops; copy and paste is more reliable.", ["drop", "transfer", "files"]),
  e("T-02", "clipboard", "Copy and paste files between PCs", "Large files wait for Fetch (see Advanced).", ["files", "transfer", "clipboard"]),
  e("T-03", "clipboard", "Match pointer style on the Linked PC", undefined, ["cursor", "scheme", "mouse"]),
  e("F-01", "clipboard", "Save received files to", "Used for phone downloads, screenshots, and large files from another PC. Default: Downloads\\AnyLeap.", ["folder", "download", "save", "path", "directory", "transfer"]),
  e("F-02", "clipboard", "Ask where to save each time", "Off: saves straight to your folder without asking. Transfers from another PC always use your folder.", ["prompt", "dialog", "ask", "download", "save"]),
  e("F-03", "clipboard", "Save files sent to phone in", "Folder on your phone for files dropped onto the mirror window or pasted images. Default: /sdcard/Download.", ["phone", "upload", "android", "sdcard", "push", "mirror"]),

  // Audio, media & power
  e("T-06", "audio", "Play Linked PC audio on this PC", undefined, ["sound", "bridge", "speakers"]),
  e("T-07", "audio", "Silence the Linked PC's speakers meanwhile", "Prevents echo. Restored on stop or after a crash.", ["mute", "echo"]),
  e("T-08", "audio", "Volume", "Volume of the Linked PC audio played on this PC.", ["sound", "level", "loudness"]),
  e("T-09", "audio", "Media keys control the PC that is playing", undefined, ["play", "pause", "keyboard"]),
  e("T-10", "audio", "Lock both PCs together", undefined, ["session", "security"]),
  e("T-11", "audio", "Sleep both PCs together", undefined, ["session", "power", "suspend"]),
  e("T-12", "audio", 'Add "Open on Linked PC" to the right-click menu', undefined, ["explorer", "context menu", "shell"]),

  // Advanced
  e("T-04", "advanced", "Large files: save to Downloads above", "Files above this size (MB) are saved directly to your receive folder.", ["limit", "size", "mb", "transfer"]),
  e("T-05", "advanced", "Large files: ask before copying above", "Bigger copies wait until you click Fetch. Default 100 MB.", ["limit", "size", "mb", "fetch", "clipboard"]),
  e("K-09", "advanced", "Record a diagnostics log", "Records input decisions and slow events without window titles or filenames. Rotates at 5 MB and keeps logs up to 7 days.", ["debug", "log", "support", "troubleshoot"]),
  e("K-10", "advanced", "Open log folder", undefined, ["debug", "log", "folder"]),
  e("K-12", "advanced", "Delete logs", "Deletes saved diagnostics log files from your PC.", ["debug", "log", "delete", "clear", "privacy", "remove"]),

  // About
  e("A-01", "about", "Privacy", "AnyLeap runs locally on your computers and phone. It has no account, no cloud, and no tracking — only optional GitHub update checks use the internet.", ["privacy", "policy", "data", "folder", "logs", "delete", "tracking", "telemetry", "about"]),
];

const BY_ID = new Map(REGISTRY.map((x) => [x.id, x]));

/** Copy for a control; throws on an unknown id so a typo fails loudly in tests. */
export function entry(id: string): SettingEntry {
  const found = BY_ID.get(id);
  if (!found) throw new Error(`Unknown setting id ${id}`);
  return found;
}
