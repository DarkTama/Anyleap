import { useId } from "react";
import { useKvmStore } from "@/stores/kvmStore";
import { entry } from "./registry";
import { Panel } from "./Panel";
import { SwitchRow } from "./SwitchRow";
import type { ShowFn } from "./SettingsShell";

/** Audio, media & power (T-06..T-12). */
export function AudioMediaPowerSection({ show }: { show: ShowFn }) {
  const kvm = useKvmStore((s) => s.settings);
  const update = useKvmStore((s) => s.updateSettings);
  const audioState = useKvmStore((s) => s.audioState);
  const volId = useId();
  const unsupported = audioState.linkedMute === "unsupported";

  return (
    <Panel>
      {show("T-06") && (
        <SwitchRow
          label={entry("T-06").label}
          hint={audioState.streaming ? "Streaming now." : undefined}
          checked={kvm.continuityAudioBridge}
          onChange={(v) => update({ continuityAudioBridge: v })}
        />
      )}
      {show("T-06") && audioState.streaming && (
        <div
          aria-hidden="true"
          className="h-1.5 w-full max-w-xs overflow-hidden rounded-full border border-ink-700 bg-ink-950"
        >
          <div
            className="progress-bar-fill h-full w-full bg-gold-400"
            style={{ transform: `scaleX(${Math.max(0, Math.min(1, audioState.level))})` }}
          />
        </div>
      )}
      {show("T-07") && (
        <SwitchRow
          label={entry("T-07").label}
          hint={entry("T-07").helper}
          checked={kvm.continuityMuteLinkedWhileBridging}
          disabled={unsupported}
          disabledReason="Not supported on the Linked PC."
          onChange={(v) => update({ continuityMuteLinkedWhileBridging: v })}
        />
      )}
      {show("T-08") && (
        <div className="space-y-1 py-1">
          <div className="flex items-center justify-between text-sm">
            <label htmlFor={volId} className="text-text-strong">
              {entry("T-08").label}
            </label>
            <output htmlFor={volId} className="font-mono font-semibold text-gold-400">
              {kvm.continuityAudioVolume}%
            </output>
          </div>
          <input
            id={volId}
            type="range"
            min={0}
            max={100}
            step={1}
            value={kvm.continuityAudioVolume}
            aria-valuetext={`${kvm.continuityAudioVolume}%`}
            aria-describedby={`${volId}-hint`}
            onChange={(e) => update({ continuityAudioVolume: Number(e.target.value) })}
            className="w-full cursor-pointer accent-gold-400"
          />
          <p id={`${volId}-hint`} className="text-[11px] text-text-muted">
            {entry("T-08").helper}
          </p>
        </div>
      )}
      {show("T-09") && (
        <SwitchRow
          label={entry("T-09").label}
          checked={kvm.continuityMediaKeysFollowAudio}
          onChange={(v) => update({ continuityMediaKeysFollowAudio: v })}
        />
      )}
      {show("T-10") && (
        <SwitchRow
          label={entry("T-10").label}
          checked={kvm.continuityLockTogether}
          onChange={(v) => update({ continuityLockTogether: v })}
        />
      )}
      {show("T-11") && (
        <SwitchRow
          label={entry("T-11").label}
          checked={kvm.continuitySleepTogether}
          onChange={(v) => update({ continuitySleepTogether: v })}
        />
      )}
      {show("T-12") && (
        <SwitchRow
          label={entry("T-12").label}
          checked={kvm.continuityOpenOnLinkedMenu}
          onChange={(v) => update({ continuityOpenOnLinkedMenu: v })}
        />
      )}
    </Panel>
  );
}
