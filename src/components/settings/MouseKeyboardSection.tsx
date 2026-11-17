import { useKvmStore } from "@/stores/kvmStore";
import { entry } from "./registry";
import { Panel } from "./Panel";
import { ReadOnlyLinkRow, SelectField } from "./FieldRows";
import { SwitchRow } from "./SwitchRow";
import type { ShowFn } from "./SettingsShell";

const CORNERS = [
  { value: "top-right", label: "Top right" },
  { value: "top-left", label: "Top left" },
  { value: "bottom-right", label: "Bottom right" },
  { value: "bottom-left", label: "Bottom left" },
];

/**
 * Shared mouse & keyboard (K-01..K-05). Crossing delay and corner dead zone are read-only
 * here; the Computers tab is their single editor (SPEC-28 §9.2).
 */
export function MouseKeyboardSection({
  show,
  onOpenComputers,
}: {
  show: ShowFn;
  onOpenComputers?: () => void;
}) {
  const kvm = useKvmStore((s) => s.settings);
  const update = useKvmStore((s) => s.updateSettings);

  return (
    <Panel>
      {show("K-01") && (
        <ReadOnlyLinkRow
          label={entry("K-01").label}
          hint={entry("K-01").helper}
          value={`${kvm.edgeResistanceMs} ms`}
          linkLabel="Edit in Computers"
          onLink={onOpenComputers}
        />
      )}
      {show("K-02") && (
        <ReadOnlyLinkRow
          label={entry("K-02").label}
          hint={entry("K-02").helper}
          value={`${kvm.cornerDeadzonePx} px`}
          linkLabel="Edit in Computers"
          onLink={onOpenComputers}
        />
      )}
      {show("K-03") && (
        <SwitchRow
          label={entry("K-03").label}
          hint={entry("K-03").helper}
          checked={kvm.indicatorHud}
          onChange={(v) => update({ indicatorHud: v })}
        />
      )}
      {show("K-04") && kvm.indicatorHud && (
        <SelectField
          label={entry("K-04").label}
          value={kvm.indicatorHudCorner}
          options={CORNERS}
          onChange={(v) => update({ indicatorHudCorner: v as typeof kvm.indicatorHudCorner })}
        />
      )}
      {show("K-05") && (
        <SwitchRow
          label={entry("K-05").label}
          hint={entry("K-05").helper}
          checked={kvm.indicatorCanvasDot}
          onChange={(v) => update({ indicatorCanvasDot: v })}
        />
      )}
      {show("K-11") && (
        <div className="space-y-1 pt-2">
          <div className="text-sm text-text-strong">{entry("K-11").label}</div>
          <div className="rounded-lg border border-ink-700 bg-ink-950 px-3 py-2 font-mono text-xs text-text-strong">
            Scroll Lock, Pause, or Ctrl+Alt+Esc
          </div>
          <p className="text-[11px] text-text-muted">{entry("K-11").helper}</p>
        </div>
      )}
    </Panel>
  );
}
