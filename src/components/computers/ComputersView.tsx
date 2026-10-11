import { Monitor, ArrowLeftRight, Shield, Zap, Sparkles } from "lucide-react";

export function ComputersView() {
  return (
    <div className="flex h-full flex-1 flex-col overflow-y-auto bg-[#0c0c0e] p-6 text-zinc-100 space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between border-b border-[#222226] pb-4">
        <div>
          <h1 className="text-base font-semibold text-zinc-100 flex items-center gap-2">
            Computer Link (Virtual KVM)
            <span className="rounded bg-cyan-500/10 px-2 py-0.5 font-mono text-[10px] font-medium text-cyan-400 border border-cyan-500/20">
              SPEC-12 Foundation
            </span>
          </h1>
          <p className="mt-0.5 text-xs text-zinc-400">
            Roam your mouse cursor and keyboard seamlessly across multiple personal computers over local Wi-Fi or USB tethering.
          </p>
        </div>

        <button
          type="button"
          className="btn-tactile flex items-center gap-1.5 rounded-lg border border-[#27272a] bg-[#16161a] px-3 py-1.5 text-xs font-medium text-zinc-300 hover:border-zinc-600 hover:bg-[#1c1c21] cursor-pointer"
        >
          <Sparkles className="h-3.5 w-3.5 text-cyan-400" />
          <span>Discover Computers</span>
        </button>
      </div>

      {/* Spatial Topology Mock Canvas */}
      <div className="rounded-xl border border-[#222226] bg-[#141417] p-5 space-y-4">
        <div className="flex items-center justify-between text-xs">
          <span className="font-mono text-[11px] font-bold uppercase tracking-wider text-zinc-400">
            Spatial Monitor Topology
          </span>
          <span className="text-[11px] text-zinc-500">
            Drag to arrange monitor boundaries
          </span>
        </div>

        {/* 2D Canvas Representation */}
        <div className="relative flex h-52 w-full items-center justify-center rounded-lg border border-dashed border-[#27272a] bg-[#0f0f12] p-4">
          <div className="flex items-center gap-6">
            {/* Primary PC Screen */}
            <div className="flex h-32 w-48 flex-col items-center justify-center rounded-lg border-2 border-cyan-500/80 bg-[#161b26] p-3 shadow-lg">
              <Monitor className="h-6 w-6 text-cyan-400" />
              <span className="mt-2 text-xs font-semibold text-zinc-100">Primary Desktop</span>
              <span className="font-mono text-[10px] text-zinc-400">2560x1440 (Host)</span>
            </div>

            {/* Edge Leap Indicator */}
            <div className="flex flex-col items-center gap-1 text-zinc-500">
              <ArrowLeftRight className="h-5 w-5 text-cyan-400 animate-pulse" />
              <span className="font-mono text-[9px] uppercase tracking-wider text-cyan-400 font-bold">Edge Leap</span>
            </div>

            {/* Secondary PC Screen */}
            <div className="flex h-28 w-40 flex-col items-center justify-center rounded-lg border border-[#3f3f46] bg-[#16161a] p-3 opacity-90">
              <Monitor className="h-5 w-5 text-zinc-400" />
              <span className="mt-2 text-xs font-medium text-zinc-300">Secondary Laptop</span>
              <span className="font-mono text-[10px] text-zinc-500">1920x1080 (Client)</span>
            </div>
          </div>
        </div>
      </div>

      {/* Feature Pillars */}
      <div className="grid grid-cols-3 gap-4">
        <div className="rounded-xl border border-[#222226] bg-[#141417] p-4 space-y-2">
          <div className="grid h-8 w-8 place-items-center rounded-lg bg-cyan-500/10 text-cyan-400 border border-cyan-500/20">
            <Zap className="h-4 w-4" />
          </div>
          <h3 className="text-xs font-semibold text-zinc-200">Sub-Millisecond UDP Movement</h3>
          <p className="text-[11px] text-zinc-400 leading-relaxed">
            Raw UDP datagrams on port 4178 stream cursor deltas without TCP buffering latency.
          </p>
        </div>

        <div className="rounded-xl border border-[#222226] bg-[#141417] p-4 space-y-2">
          <div className="grid h-8 w-8 place-items-center rounded-lg bg-emerald-500/10 text-emerald-400 border border-emerald-500/20">
            <Shield className="h-4 w-4" />
          </div>
          <h3 className="text-xs font-semibold text-zinc-200">Hardware Input Injection</h3>
          <p className="text-[11px] text-zinc-400 leading-relaxed">
            Low-level Win32 hooks suppress local keystrokes and inject native inputs via SendInput.
          </p>
        </div>

        <div className="rounded-xl border border-[#222226] bg-[#141417] p-4 space-y-2">
          <div className="grid h-8 w-8 place-items-center rounded-lg bg-purple-500/10 text-purple-400 border border-purple-500/20">
            <ArrowLeftRight className="h-4 w-4" />
          </div>
          <h3 className="text-xs font-semibold text-zinc-200">Synchronized Clipboard</h3>
          <p className="text-[11px] text-zinc-400 leading-relaxed">
            Copy text on one computer and immediately paste it on another across the network.
          </p>
        </div>
      </div>
    </div>
  );
}
