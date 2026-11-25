import { getAppAvatar } from "@/lib/notificationTidy";

export interface AppAvatarProps {
  label: string;
  packageName: string;
  sizeClass?: string;
}

export function AppAvatar({
  label,
  packageName,
  sizeClass = "h-6 w-6 text-[11px]",
}: AppAvatarProps) {
  const { letter, bg, fg } = getAppAvatar(label, packageName);
  return (
    <span
      aria-hidden="true"
      style={{ backgroundColor: bg, color: fg }}
      className={`flex ${sizeClass} shrink-0 items-center justify-center rounded-md border border-ink-700 font-bold`}
    >
      {letter}
    </span>
  );
}
