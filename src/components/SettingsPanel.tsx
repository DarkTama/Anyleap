import { useEffect } from "react";
import { PhoneMirroringSection } from "@/components/settings/PhoneMirroringSection";
import { GeneralSection } from "@/components/settings/GeneralSection";
import { ControlBarSection } from "@/components/settings/ControlBarSection";
import { MouseKeyboardSection } from "@/components/settings/MouseKeyboardSection";
import { ClipboardFilesSection } from "@/components/settings/ClipboardFilesSection";
import { AudioMediaPowerSection } from "@/components/settings/AudioMediaPowerSection";
import { AdvancedSection } from "@/components/settings/AdvancedSection";
import { SettingsShell } from "@/components/settings/SettingsShell";
import type { SectionId } from "@/components/settings/registry";
import type { MirrorMode } from "@/lib/modeSettings";
import { useKvmStore } from "@/stores/kvmStore";
import { kvmSetSyncSettings } from "@/services/kvmService";

export interface SettingsTarget {
  section: SectionId;
  mode?: MirrorMode;
  tick: number;
}

/**
 * Settings (SPEC-28 §7): seven sections behind a left rail and a filter box.
 * `onOpenComputers` switches to the Computers tab, the single editor for crossing
 * delay and corner dead zone (§9.2).
 */
export function SettingsPanel({
  onOpenComputers,
  target,
}: {
  onOpenComputers?: () => void;
  target?: SettingsTarget | null;
}) {
  const text = useKvmStore((s) => s.settings.syncClipboardText);
  const image = useKvmStore((s) => s.settings.syncClipboardImage);

  useEffect(() => {
    // Retired gate (SPEC-28 §9.3): always on; "Copy and paste files" is the only file switch.
    void kvmSetSyncSettings(text, image, true).catch(() => {});
  }, [text, image]);

  return (
    <SettingsShell
      targetSection={target ? { section: target.section, tick: target.tick } : undefined}
      sections={{
        general: ({ show }) => <GeneralSection show={show} />,
        phone: () => (
          <PhoneMirroringSection
            targetMode={
              target?.section === "phone" && target.mode
                ? { mode: target.mode, tick: target.tick }
                : undefined
            }
          />
        ),
        controlbar: ({ show }) => <ControlBarSection show={show} />,
        mousekb: ({ show }) => (
          <MouseKeyboardSection show={show} onOpenComputers={onOpenComputers} />
        ),
        clipboard: ({ show }) => <ClipboardFilesSection show={show} />,
        audio: ({ show }) => <AudioMediaPowerSection show={show} />,
        advanced: ({ show, advancedOpen }) => (
          <AdvancedSection show={show} autoOpen={advancedOpen} />
        ),
      }}
    />
  );
}
