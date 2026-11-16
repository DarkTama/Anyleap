import { useEffect, useMemo, useRef, useState } from "react";
import { History, Info, Lock, MessageSquare, Users } from "lucide-react";
import {
  phoneGetConversations,
  phoneMarkConversationRead,
} from "@/services/phoneHubService";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import type { Conversation } from "@/types/phone";

interface MessagesViewProps {
  serial: string;
}

const EMPTY_CONVERSATIONS: Conversation[] = [];

const SENDER_COLORS = [
  "text-mist-300",
  "text-gold-400",
  "text-status-success",
  "text-status-warning",
];

function senderColorClass(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash += name.charCodeAt(i);
  }
  return SENDER_COLORS[hash % SENDER_COLORS.length];
}

function formatMessageTime(ms: number): string {
  if (!ms) return "";
  return new Date(ms).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function MessagesView({ serial }: MessagesViewProps) {
  const conversations = usePhoneHubStore(
    (s) => s.conversationsBySerial[serial] ?? EMPTY_CONVERSATIONS,
  );
  const applyConversations = usePhoneHubStore((s) => s.applyConversations);
  const markConversationReadLocal = usePhoneHubStore(
    (s) => s.markConversationReadLocal,
  );

  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [newDividerCount, setNewDividerCount] = useState<number>(0);
  const threadBodyRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    setSelectedKey(null);
    setNewDividerCount(0);
    let cancelled = false;
    phoneGetConversations(serial)
      .then((list) => {
        if (!cancelled) applyConversations(serial, list);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [serial, applyConversations]);

  // Group conversations by appLabel (preserving newest-first order)
  const appGroups = useMemo(() => {
    const map = new Map<string, { label: string; items: Conversation[] }>();
    for (const c of conversations) {
      const label = c.appLabel || c.package;
      const existing = map.get(c.package);
      if (existing) {
        existing.items.push(c);
      } else {
        map.set(c.package, { label, items: [c] });
      }
    }
    return Array.from(map.entries()).map(([pkg, data]) => ({
      pkg,
      label: data.label,
      items: data.items,
    }));
  }, [conversations]);

  const selectedConversation = useMemo(
    () => conversations.find((c) => c.key === selectedKey) ?? null,
    [conversations, selectedKey],
  );

  function handleSelectConversation(c: Conversation) {
    setNewDividerCount(c.unread > 0 ? c.unread : 0);
    setSelectedKey(c.key);
    if (c.unread > 0) {
      markConversationReadLocal(serial, c.key);
      void phoneMarkConversationRead(serial, c.key).catch(() => {});
    }
    requestAnimationFrame(() => {
      if (threadBodyRef.current) {
        threadBodyRef.current.scrollTop = threadBodyRef.current.scrollHeight;
      }
    });
  }

  if (conversations.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
        <MessageSquare className="mb-2 h-10 w-10 text-ink-700" />
        <h2 className="text-sm font-semibold text-text-strong">No messages yet</h2>
        <p className="mt-1 max-w-md text-xs text-text">
          Messages appear here while your phone is connected; only messages that arrive as notifications are captured.
        </p>
        <p className="mt-1.5 max-w-md text-xs text-text-muted">
          History is kept in memory only and is cleared when the phone disconnects or AnyLeap closes.
        </p>
      </div>
    );
  }

  const uniqueSenders = selectedConversation
    ? new Set(selectedConversation.messages.map((m) => m.sender)).size
    : 0;

  return (
    <div className="flex flex-1 min-h-0 overflow-hidden">
      {/* Left: conversation list */}
      <aside
        aria-label="Conversations"
        className="flex w-[260px] shrink-0 flex-col border-r border-ink-700 bg-ink-950 xl:w-[300px]"
      >
        <div className="flex-1 overflow-y-auto p-2 space-y-3">
          {appGroups.map((group) => (
            <section key={group.pkg} aria-label={group.label}>
              <div className="flex items-center gap-1.5 px-2 py-1.5 font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
                <span>{group.label}</span>
                <span className="rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 text-[10px] font-normal text-text">
                  {group.items.length}
                </span>
              </div>
              <div className="space-y-0.5">
                {group.items.map((c) => {
                  const last = c.messages[c.messages.length - 1];
                  const isCurrent = selectedKey === c.key;
                  const isUnread = c.unread > 0;
                  return (
                    <button
                      key={c.key}
                      type="button"
                      aria-current={isCurrent ? "true" : undefined}
                      onClick={() => handleSelectConversation(c)}
                      className={`btn-tactile flex w-full items-center gap-2.5 rounded-[10px] border p-2 text-left cursor-pointer ${
                        isCurrent
                          ? "border-ink-700 bg-ink-900"
                          : "border-transparent hover:bg-ink-900"
                      }`}
                    >
                      <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-ink-700 bg-ink-800 text-[13px] font-bold text-text-strong">
                        {c.isGroup ? (
                          <Users className="h-4 w-4" />
                        ) : (
                          (c.title[0] || "?").toUpperCase()
                        )}
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="flex items-center justify-between gap-2">
                          <span className="truncate text-xs font-semibold text-text-strong">
                            {c.title}
                          </span>
                          <span className="shrink-0 font-mono text-[10px] text-text-muted">
                            {last ? formatMessageTime(last.timestampMs) : ""}
                          </span>
                        </span>
                        <span className="mt-0.5 flex items-center justify-between gap-2">
                          <span
                            className={`truncate text-[11px] ${
                              isUnread ? "text-text" : "text-text-muted"
                            }`}
                          >
                            {last
                              ? `${c.isGroup ? `${last.sender}: ` : ""}${last.text}`
                              : ""}
                          </span>
                          {isUnread && (
                            <span
                              aria-label={`${c.unread} unread`}
                              className="inline-flex min-w-[18px] shrink-0 items-center justify-center rounded-full bg-gold-400 px-1.5 py-0.5 font-mono text-[10px] font-bold leading-none text-ink-950"
                            >
                              {c.unread}
                            </span>
                          )}
                        </span>
                      </span>
                    </button>
                  );
                })}
              </div>
            </section>
          ))}
        </div>

        <div className="flex items-start gap-2 border-t border-ink-700 bg-ink-900 px-3 py-2.5 text-[11px] leading-[15px] text-text">
          <Lock className="mt-0.5 h-4 w-4 shrink-0 text-text-muted" />
          <span>
            <strong className="font-semibold text-text-strong">
              History is cleared when your phone disconnects.
            </strong>{" "}
            Kept in memory only; never saved to disk.
          </span>
        </div>
      </aside>

      {/* Right: thread view */}
      <section className="flex min-w-0 flex-1 flex-col bg-ink-950">
        {!selectedConversation ? (
          <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
            <MessageSquare className="mb-2 h-10 w-10 text-ink-700" />
            <h2 className="text-sm font-semibold text-text-strong">
              Select a conversation
            </h2>
            <p className="mt-1 text-xs text-text">
              Messages stay on this PC in memory only.
            </p>
          </div>
        ) : (
          <>
            <div className="flex items-center gap-2.5 border-b border-ink-700 bg-ink-900 px-4 py-2.5">
              <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-ink-700 bg-ink-800 text-[13px] font-bold text-text-strong">
                {selectedConversation.isGroup ? (
                  <Users className="h-4 w-4" />
                ) : (
                  (selectedConversation.title[0] || "?").toUpperCase()
                )}
              </span>
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px] font-semibold text-text-strong">
                  {selectedConversation.title}
                </div>
                <div className="text-[11px] text-text-muted">
                  {selectedConversation.appLabel || selectedConversation.package}
                  {selectedConversation.isGroup ? ` · ${uniqueSenders} senders` : ""}
                </div>
              </div>
              <span className="inline-flex items-center gap-1 rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 font-mono text-[11px] text-text">
                <History className="h-3 w-3" />
                {selectedConversation.messages.length} message
                {selectedConversation.messages.length === 1 ? "" : "s"} · in memory only
              </span>
            </div>

            <div
              ref={threadBodyRef}
              role="log"
              aria-label={`Messages in ${selectedConversation.title}`}
              tabIndex={0}
              className="flex flex-1 flex-col gap-2.5 overflow-y-auto p-4"
            >
              {selectedConversation.messages.map((m, idx) => {
                const dividerIdx = Math.max(
                  0,
                  selectedConversation.messages.length - newDividerCount,
                );
                const showDivider = newDividerCount > 0 && idx === dividerIdx;
                return (
                  <div key={`${m.timestampMs}-${m.sender}-${idx}`} className="contents">
                    {showDivider && (
                      <div
                        role="separator"
                        className="flex items-center gap-2.5 font-mono text-[11px] font-semibold text-gold-400"
                      >
                        <span className="flex-1 border-t border-gold-600" />
                        <span>
                          {newDividerCount} new message{newDividerCount > 1 ? "s" : ""}
                        </span>
                        <span className="flex-1 border-t border-gold-600" />
                      </div>
                    )}
                    <div className="max-w-[min(520px,85%)]">
                      <div
                        className={`mb-0.5 text-[11px] font-semibold ${senderColorClass(m.sender)}`}
                      >
                        {m.sender}
                      </div>
                      <div className="rounded-[4px_12px_12px_12px] border border-ink-700 bg-ink-900 px-3 py-2 text-xs leading-[17px] text-text-strong whitespace-pre-wrap break-words">
                        {m.text}
                      </div>
                      <div className="mt-0.5 font-mono text-[10px] text-text-muted">
                        {formatMessageTime(m.timestampMs)}
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>

            <div className="flex items-center gap-2 border-t border-ink-700 bg-ink-900 px-4 py-2 text-xs text-text-muted">
              <Info className="h-3.5 w-3.5 shrink-0" />
              <span>Read-only — replies need the phone.</span>
            </div>
          </>
        )}
      </section>
    </div>
  );
}
