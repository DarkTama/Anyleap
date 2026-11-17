import { useKvmStore } from "@/stores/kvmStore";
import { entry } from "./registry";
import { Panel } from "./Panel";
import { SwitchRow } from "./SwitchRow";
import type { ShowFn } from "./SettingsShell";

/** Clipboard & files (K-06, K-07, T-01..T-03). */
export function ClipboardFilesSection({ show }: { show: ShowFn }) {
  const kvm = useKvmStore((s) => s.settings);
  const update = useKvmStore((s) => s.updateSettings);
  return (
    <Panel>
      {show("K-06") && (
        <SwitchRow
          label={entry("K-06").label}
          hint={entry("K-06").helper}
          checked={kvm.syncClipboardText}
          onChange={(v) => update({ syncClipboardText: v })}
        />
      )}
      {show("K-07") && (
        <SwitchRow
          label={entry("K-07").label}
          hint={entry("K-07").helper}
          checked={kvm.syncClipboardImage}
          onChange={(v) => update({ syncClipboardImage: v })}
        />
      )}
      {show("T-01") && (
        <SwitchRow
          label={entry("T-01").label}
          hint={entry("T-01").helper}
          checked={kvm.continuityDragDropExperimental}
          onChange={(v) => update({ continuityDragDropExperimental: v })}
        />
      )}
      {show("T-02") && (
        <SwitchRow
          label={entry("T-02").label}
          hint={entry("T-02").helper}
          checked={kvm.continuityFileClipboard}
          onChange={(v) => update({ continuityFileClipboard: v })}
        />
      )}
      {show("T-03") && (
        <SwitchRow
          label={entry("T-03").label}
          checked={kvm.continuityPointerScheme}
          onChange={(v) => update({ continuityPointerScheme: v })}
        />
      )}
    </Panel>
  );
}
