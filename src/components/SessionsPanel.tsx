import { useState } from "react";
import { Square } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/useAppStore";
import { stopMirror } from "@/lib/tauri";

export function SessionsPanel() {
  const sessions = useAppStore((s) => s.sessions);
  const setError = useAppStore((s) => s.setError);
  const [stopping, setStopping] = useState<string | null>(null);

  async function stop(id: string) {
    setStopping(id);
    try {
      await stopMirror(id);
    } catch (e) {
      setError(String(e));
    } finally {
      setStopping(null);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Running sessions</CardTitle>
      </CardHeader>
      <CardContent>
        {sessions.length === 0 ? (
          <p className="text-sm text-zinc-500">No active mirrors.</p>
        ) : (
          <ul className="space-y-2">
            {sessions.map((s) => (
              <li
                key={s.id}
                className="flex items-center justify-between rounded-md border border-zinc-100 px-3 py-2 text-sm dark:border-zinc-800/60"
              >
                <span className="font-mono text-xs">
                  {s.serial} · pid {s.pid}
                </span>
                <Button
                  size="sm"
                  variant="destructive"
                  onClick={() => stop(s.id)}
                  disabled={stopping === s.id}
                >
                  <Square className="h-3.5 w-3.5" />
                  {stopping === s.id ? "Stopping…" : "Stop"}
                </Button>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
