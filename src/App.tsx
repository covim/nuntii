import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { AccountSetupDialog } from "@/features/accounts/AccountSetupDialog";
import { ComposeDialog, type ComposeRequest } from "@/features/compose/ComposeDialog";
import { Sidebar } from "@/features/folders/Sidebar";
import { MessageList } from "@/features/messages/MessageList";
import { ReadingPane, type ComposeMode } from "@/features/messages/ReadingPane";
import { SettingsDialog } from "@/features/settings/SettingsDialog";
import { api, errorMessage, type Folder, type ListFilter, type MessageDetail } from "@/lib/api";
import { keys, useAccounts, useBackendEvents, useFolders, useMessages, useSearch } from "@/lib/queries";
import { queryClient } from "@/lib/queries";

function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

function isTyping(e: KeyboardEvent): boolean {
  const el = e.target as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
}

export default function App() {
  useBackendEvents();
  const { data: accounts, isFetched: accountsLoaded } = useAccounts();
  const { data: folders = [] } = useFolders();
  const [folderId, setFolderId] = useState<number | null>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<ListFilter>("all");
  const debouncedSearch = useDebounced(search.trim(), 200);
  const [compose, setCompose] = useState<ComposeRequest | null>(null);
  const [setupOpen, setSetupOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);

  // First start: open the account setup.
  useEffect(() => {
    if (accountsLoaded && accounts?.length === 0) setSetupOpen(true);
  }, [accountsLoaded, accounts?.length]);

  // Default to the first inbox; drop the selection if the folder disappeared.
  useEffect(() => {
    if (folders.length === 0) return;
    if (folderId == null || !folders.some((f) => f.id === folderId)) {
      setFolderId((folders.find((f) => f.role === "inbox") ?? folders[0]).id);
    }
  }, [folders, folderId]);

  const folder = folders.find((f) => f.id === folderId) ?? null;
  const list = useMessages(folderId, filter, selectedId);
  const searchResults = useSearch(debouncedSearch);
  const messages = useMemo(
    () => {
      if (!debouncedSearch) return list.data?.pages.flat() ?? [];
      const results = searchResults.data ?? [];
      if (filter === "unread") return results.filter((m) => !m.seen || m.id === selectedId);
      if (filter === "flagged") return results.filter((m) => m.flagged || m.id === selectedId);
      return results;
    },
    [debouncedSearch, searchResults.data, list.data, filter, selectedId],
  );
  const selected = messages.find((m) => m.id === selectedId) ?? null;

  const selectFolder = (f: Folder) => {
    setFolderId(f.id);
    setSelectedId(null);
    setSearch("");
  };

  const move = useCallback(
    (delta: number) => {
      if (messages.length === 0) return;
      const idx = messages.findIndex((m) => m.id === selectedId);
      const next = Math.min(Math.max(idx + delta, 0), messages.length - 1);
      setSelectedId(messages[idx < 0 ? 0 : next].id);
    },
    [messages, selectedId],
  );

  // After delete/move: select the neighbour so triage can continue with the keyboard.
  const onRemoved = useCallback(() => {
    const idx = messages.findIndex((m) => m.id === selectedId);
    const neighbour = messages[idx + 1] ?? messages[idx - 1];
    setSelectedId(neighbour?.id ?? null);
  }, [messages, selectedId]);

  const openCompose = useCallback((mode: ComposeMode, source: MessageDetail) => setCompose({ mode, source }), []);
  const onMailto = useCallback((to: string) => setCompose({ mode: "new", to }), []);

  const composeFromSelection = useCallback(
    async (mode: ComposeMode) => {
      if (!selected) return;
      try {
        const detail = await queryClient.fetchQuery({
          queryKey: keys.message(selected.id, false),
          queryFn: () => api.messageGet(selected.id, false),
        });
        setCompose({ mode, source: detail });
      } catch (e) {
        toast.error(errorMessage(e));
      }
    },
    [selected],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e) || e.ctrlKey || e.metaKey || e.altKey || compose || setupOpen || settingsOpen) return;
      const run = (p: Promise<unknown>) => p.catch((err) => toast.error(errorMessage(err)));
      switch (e.key) {
        case "j":
        case "ArrowDown":
          move(1);
          break;
        case "k":
        case "ArrowUp":
          move(-1);
          break;
        case "/":
          searchRef.current?.focus();
          break;
        case "c":
          if (accounts?.length) setCompose({ mode: "new" });
          break;
        case "r":
          composeFromSelection("reply");
          break;
        case "a":
          composeFromSelection("replyAll");
          break;
        case "f":
          composeFromSelection("forward");
          break;
        case "s":
          if (selected) run(api.messagesSetFlag([selected.id], "flagged", !selected.flagged));
          break;
        case "u":
          if (selected) run(api.messagesSetFlag([selected.id], "seen", !selected.seen));
          break;
        case "Delete":
          if (selected) run(api.messagesDelete([selected.id]).then(onRemoved));
          break;
        default:
          return;
      }
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [move, composeFromSelection, selected, compose, setupOpen, settingsOpen, accounts?.length, onRemoved]);

  const showRecipients = folder?.role === "sent" || folder?.role === "drafts";

  return (
    <div className="h-screen w-screen overflow-hidden bg-background text-foreground">
      <ResizablePanelGroup orientation="horizontal" id="nuntii-layout">
        <ResizablePanel defaultSize="18" minSize={180} maxSize="35">
          <Sidebar
            selectedFolderId={debouncedSearch ? null : folderId}
            onSelectFolder={selectFolder}
            onCompose={() => setCompose({ mode: "new" })}
            onOpenDraft={(draft) => setCompose({ mode: draft.mode, draft })}
            onAddAccount={() => setSetupOpen(true)}
            onOpenSettings={() => setSettingsOpen(true)}
          />
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel defaultSize="30" minSize={260}>
          <MessageList
            ref={searchRef}
            title={folder?.displayName ?? ""}
            messages={messages}
            loading={debouncedSearch ? searchResults.isFetching : list.isFetching}
            showRecipients={!debouncedSearch && showRecipients}
            selectedId={selectedId}
            onSelect={(m) => setSelectedId(m.id)}
            hasMore={!debouncedSearch && !!list.hasNextPage}
            onLoadMore={() => {
              if (!list.isFetchingNextPage) list.fetchNextPage();
            }}
            searchQuery={search}
            onSearchChange={setSearch}
            filter={filter}
            onFilterChange={setFilter}
          />
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel defaultSize="52" minSize={320}>
          <ReadingPane message={selected} onCompose={openCompose} onMailto={onMailto} onDeleted={onRemoved} />
        </ResizablePanel>
      </ResizablePanelGroup>

      <ComposeDialog request={compose} onClose={() => setCompose(null)} />
      <AccountSetupDialog open={setupOpen} onOpenChange={setSetupOpen} />
      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
    </div>
  );
}
