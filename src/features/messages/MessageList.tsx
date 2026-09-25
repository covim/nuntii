import { forwardRef, useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Flag, Paperclip, Reply, Search, X } from "lucide-react";
import { Input } from "@/components/ui/input";
import type { ListFilter, MessageSummary } from "@/lib/api";
import { addressList, addressName, formatListDate } from "@/lib/format";
import { cn } from "@/lib/utils";

const ROW_HEIGHT = 76;

interface MessageListProps {
  title: string;
  messages: MessageSummary[];
  loading: boolean;
  /** Show recipients instead of senders (sent/drafts). */
  showRecipients: boolean;
  selectedId: number | null;
  onSelect: (m: MessageSummary) => void;
  hasMore: boolean;
  onLoadMore: () => void;
  searchQuery: string;
  onSearchChange: (q: string) => void;
  filter: ListFilter;
  onFilterChange: (f: ListFilter) => void;
}

const FILTERS: { value: ListFilter; label: string; empty: string }[] = [
  { value: "all", label: "Alle", empty: "Keine Nachrichten" },
  { value: "unread", label: "Ungelesen", empty: "Keine ungelesenen Nachrichten" },
  { value: "flagged", label: "Markiert", empty: "Keine markierten Nachrichten" },
];

export const MessageList = forwardRef<HTMLInputElement, MessageListProps>(function MessageList(
  { title, messages, loading, showRecipients, selectedId, onSelect, hasMore, onLoadMore, searchQuery, onSearchChange, filter, onFilterChange },
  searchRef,
) {
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  const items = virtualizer.getVirtualItems();
  const lastIndex = items.length ? items[items.length - 1].index : -1;
  useEffect(() => {
    if (hasMore && lastIndex >= messages.length - 20) onLoadMore();
  }, [lastIndex, messages.length, hasMore, onLoadMore]);

  // Keep keyboard selection visible.
  useEffect(() => {
    const idx = messages.findIndex((m) => m.id === selectedId);
    if (idx >= 0) virtualizer.scrollToIndex(idx, { align: "auto" });
  }, [selectedId, messages, virtualizer]);

  return (
    <div className="flex h-full flex-col">
      <div className="space-y-2 border-b p-3">
        <h1 className="truncate text-base font-semibold">{searchQuery ? "Suchergebnisse" : title}</h1>
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            ref={searchRef}
            value={searchQuery}
            onChange={(e) => onSearchChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                onSearchChange("");
                e.currentTarget.blur();
              }
            }}
            placeholder="Suchen (/)  –  z. B. from:anna rechnung"
            className="pr-8 pl-8"
            aria-label="Mails durchsuchen"
          />
          {searchQuery && (
            <button
              type="button"
              className="absolute top-1/2 right-2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              onClick={() => onSearchChange("")}
              aria-label="Suche löschen"
            >
              <X className="size-4" />
            </button>
          )}
        </div>
        <div className="flex gap-1" role="radiogroup" aria-label="Filter">
          {FILTERS.map((f) => (
            <button
              key={f.value}
              type="button"
              role="radio"
              aria-checked={filter === f.value}
              onClick={() => onFilterChange(f.value)}
              className={cn(
                "rounded-md px-2.5 py-1 text-xs font-medium transition-colors",
                filter === f.value ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground",
              )}
            >
              {f.label}
            </button>
          ))}
        </div>
      </div>

      <div ref={parentRef} className="min-h-0 flex-1 overflow-y-auto" role="listbox" aria-label="Nachrichten">
        {messages.length === 0 && (
          <div className="p-6 text-center text-sm text-muted-foreground">
            {loading ? "Lädt…" : searchQuery ? "Keine Treffer" : FILTERS.find((f) => f.value === filter)!.empty}
          </div>
        )}
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {items.map((row) => {
            const m = messages[row.index];
            const who = showRecipients ? `An: ${addressList(m.to)}` : addressName(m.from[0]);
            const active = m.id === selectedId;
            return (
              <div
                key={m.id}
                role="option"
                aria-selected={active}
                onClick={() => onSelect(m)}
                style={{ position: "absolute", top: 0, left: 0, right: 0, height: ROW_HEIGHT, transform: `translateY(${row.start}px)` }}
                className={cn(
                  "flex cursor-default flex-col justify-center gap-0.5 border-b px-4 text-sm select-none",
                  active ? "bg-accent" : "hover:bg-muted/60",
                )}
              >
                <div className="flex items-center gap-2">
                  {!m.seen && <span className="size-2 shrink-0 rounded-full bg-blue-500" aria-label="Ungelesen" />}
                  <span className={cn("flex-1 truncate", !m.seen && "font-semibold")}>{who}</span>
                  {m.answered && <Reply className="size-3.5 shrink-0 text-muted-foreground" aria-label="Beantwortet" />}
                  {m.hasAttachments && <Paperclip className="size-3.5 shrink-0 text-muted-foreground" aria-label="Anhang" />}
                  {m.flagged && <Flag className="size-3.5 shrink-0 fill-orange-500 text-orange-500" aria-label="Markiert" />}
                  <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{formatListDate(m.date)}</span>
                </div>
                <div className={cn("truncate", !m.seen ? "font-medium" : "text-foreground/90")}>
                  {m.subject || "(kein Betreff)"}
                </div>
                <div className="truncate text-xs text-muted-foreground">{m.snippet || " "}</div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
});
