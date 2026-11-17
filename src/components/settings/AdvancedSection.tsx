import { useEffect, useId, useState } from "react";
import { ChevronDown, FolderOpen } from "lucide-react";
import { useKvmStore } from "@/stores/kvmStore";
import { kvmGetDebugLogPath, kvmOpenDebugLogFolder } from "@/services/kvmService";
import { entry } from "./registry";
import { Panel } from "./Panel";
import { NumberField } from "./FieldRows";
import { SwitchRow } from "./SwitchRow";
import type { ShowFn } from "./SettingsShell";

/**
 * Advanced (T-04, T-05, K-09, K-10): collapsed by default; auto-expands while a filter
 * matches one of its rows (SPEC-28 §7). Last in the focus order.
 */
export function AdvancedSection({ show, autoOpen }: { show: ShowFn; autoOpen: boolean }) {
  const kvm = useKvmStore((s) => s.settings);
  const update = useKvmStore((s) => s.updateSettings);
  const [userOpen, setUserOpen] = useState(false);
  const [logPath, setLogPath] = useState("");
  const bodyId = useId();
  const open = userOpen || autoOpen;

  useEffect(() => {
    void kvmGetDebugLogPath()
      .then((p) => setLogPath(p))
      .catch(() => {});
  }, []);

  const toggleLog = (enabled: boolean) => {
    update({ debugLog: enabled });
    if (!logPath) {
      void kvmGetDebugLogPath()
        .then((p) => setLogPath(p))
        .catch(() => {});
    }
  };

  return (
    <Panel className="space-y-2">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={bodyId}
        onClick={() => setUserOpen((v) => !v)}
        className="flex min-h-8 w-full cursor-pointer items-center justify-between rounded text-left text-sm font-medium text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
      >
        <span>Limits and diagnostics</span>
        <ChevronDown
          aria-hidden="true"
          className={`h-4 w-4 transition-transform motion-reduce:transition-none ${open ? "rotate-180" : ""}`}
        />
      </button>
      <div id={bodyId} hidden={!open} className="space-y-1">
        {show("T-04") && (
          <NumberField
            label={entry("T-04").label}
            hint={entry("T-04").helper}
            value={kvm.continuityStagingLimitMb}
            min={64}
            onChange={(v) => update({ continuityStagingLimitMb: Math.max(64, v) })}
          />
        )}
        {show("T-05") && (
          <NumberField
            label={entry("T-05").label}
            hint={entry("T-05").helper}
            value={kvm.continuityClipPrefetchLimitMb}
            onChange={(v) => update({ continuityClipPrefetchLimitMb: Math.max(0, v) })}
          />
        )}
        {show("K-09") && (
          <SwitchRow
            label={entry("K-09").label}
            hint={entry("K-09").helper}
            checked={kvm.debugLog ?? false}
            onChange={toggleLog}
          />
        )}
        {show("K-10") && (
          <div className="flex items-center gap-2 pt-1">
            <div className="min-w-0 flex-1 truncate rounded-lg border border-ink-700 bg-ink-950 px-3 py-1.5 font-mono text-[11px] text-text">
              {logPath || "kvm-debug.log"}
            </div>
            <button
              type="button"
              onClick={() => void kvmOpenDebugLogFolder().catch(() => {})}
              className="btn-tactile flex min-h-8 shrink-0 cursor-pointer items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
            >
              <FolderOpen aria-hidden="true" className="h-3.5 w-3.5" />
              <span>{entry("K-10").label}</span>
            </button>
          </div>
        )}
      </div>
    </Panel>
  );
}
