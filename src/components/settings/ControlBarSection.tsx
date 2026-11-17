import { emit } from "@tauri-apps/api/event";
import { useAppStore } from "@/store/useAppStore";
import {
  BUTTON_IDS,
  BUTTON_LABELS,
  CONTROL_SIZES,
  DOCK_SIDES,
  saveControlConfig,
  type ControlConfig,
} from "@/lib/controlConfig";
import { entry } from "./registry";
import { Panel } from "./Panel";
import { SelectField } from "./FieldRows";
import type { ShowFn } from "./SettingsShell";

const SIZE_LABELS: Record<ControlConfig["size"], string> = {
  sm: "Small",
  md: "Medium",
  lg: "Large",
};

/** Control bar (C-01..C-03): moved unchanged; Phase 3 regroups the buttons. */
export function ControlBarSection({ show }: { show: ShowFn }) {
  const controlConfig = useAppStore((s) => s.controlConfig);
  const setControlConfig = useAppStore((s) => s.setControlConfig);

  function updateControl(next: ControlConfig) {
    setControlConfig(next);
    void saveControlConfig(next);
    void emit("control-config", next); // live-update the floating control window
  }

  return (
    <Panel className="space-y-3">
      {(show("C-01") || show("C-02")) && (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {show("C-01") && (
            <SelectField
              label={entry("C-01").label}
              value={controlConfig.dock}
              options={DOCK_SIDES.map((d) => ({
                value: d,
                label: d === "undocked" ? "Undocked (free)" : d[0].toUpperCase() + d.slice(1),
              }))}
              onChange={(v) => updateControl({ ...controlConfig, dock: v as ControlConfig["dock"] })}
            />
          )}
          {show("C-02") && (
            <SelectField
              label={entry("C-02").label}
              value={controlConfig.size}
              options={CONTROL_SIZES.map((s) => ({ value: s, label: SIZE_LABELS[s] }))}
              onChange={(v) => updateControl({ ...controlConfig, size: v as ControlConfig["size"] })}
            />
          )}
        </div>
      )}

      {show("C-03") && (
        <fieldset aria-describedby="control-buttons-hint" className="space-y-1">
          <legend className="text-sm text-text-strong">{entry("C-03").label}</legend>
          <p id="control-buttons-hint" className="text-[11px] text-text-muted">
            {entry("C-03").helper}
          </p>
          <div className="grid grid-cols-1 gap-x-4 sm:grid-cols-2">
            {BUTTON_IDS.map((id) => (
              <label
                key={id}
                className="flex min-h-8 cursor-pointer items-center gap-2 text-sm text-text"
              >
                <input
                  type="checkbox"
                  className="h-4 w-4 accent-gold-400"
                  checked={controlConfig.buttons[id]}
                  onChange={(e) =>
                    updateControl({
                      ...controlConfig,
                      buttons: { ...controlConfig.buttons, [id]: e.target.checked },
                    })
                  }
                />
                {BUTTON_LABELS[id]}
              </label>
            ))}
          </div>
        </fieldset>
      )}

      <p className="text-[11px] text-text-muted">
        Applies to the floating control strip and the in-app controls; changes take effect
        immediately.
      </p>
    </Panel>
  );
}
