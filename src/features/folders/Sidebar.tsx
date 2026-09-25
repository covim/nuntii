import { useMemo } from "react";
import {
  AlertTriangle,
  Archive,
  File,
  FilePen,
  Folder as FolderIcon,
  Inbox,
  Loader2,
  PenSquare,
  Plus,
  RefreshCw,
  Send,
  Settings,
  ShieldAlert,
  Trash2,
  Upload,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { api, type Account, type Folder, type FolderRole, type LocalDraft } from "@/lib/api";
import { formatListDate } from "@/lib/format";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useAccountStatus, useAccounts, useDrafts, useFolders, useOutbox, useSyncProgress } from "@/lib/queries";
import { cn } from "@/lib/utils";

const roleIcon: Record<FolderRole, typeof Inbox> = {
  inbox: Inbox,
  sent: Send,
  drafts: File,
  trash: Trash2,
  archive: Archive,
  junk: ShieldAlert,
};

function folderDepth(f: Folder): number {
  if (!f.delimiter || f.role) return 0;
  return f.remoteName.split(f.delimiter).length - 1;
}

function leafName(f: Folder): string {
  if (!f.delimiter || f.role === "inbox") return f.displayName;
  const parts = f.displayName.split(f.delimiter);
  return parts[parts.length - 1] || f.displayName;
}

interface SidebarProps {
  selectedFolderId: number | null;
  onSelectFolder: (folder: Folder) => void;
  onCompose: () => void;
  onOpenDraft: (draft: LocalDraft) => void;
  onAddAccount: () => void;
  onOpenSettings: () => void;
}

export function Sidebar({ selectedFolderId, onSelectFolder, onCompose, onOpenDraft, onAddAccount, onOpenSettings }: SidebarProps) {
  const { data: drafts = [] } = useDrafts();
  const { data: accounts = [] } = useAccounts();
  const { data: folders = [] } = useFolders();
  const { data: status } = useAccountStatus();
  const { data: progress } = useSyncProgress();
  const { data: outbox = [] } = useOutbox();

  const byAccount = useMemo(() => {
    const map = new Map<number, Folder[]>();
    for (const f of folders) {
      if (!map.has(f.accountId)) map.set(f.accountId, []);
      map.get(f.accountId)!.push(f);
    }
    return map;
  }, [folders]);

  return (
    <div className="flex h-full flex-col bg-sidebar text-sidebar-foreground">
      <div className="flex items-center gap-2 p-3">
        <Button className="flex-1 justify-start" onClick={onCompose} disabled={accounts.length === 0}>
          <PenSquare /> Neue Nachricht
        </Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" aria-label="Alle Konten synchronisieren" onClick={() => api.syncNow(null)}>
              <RefreshCw />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Jetzt synchronisieren</TooltipContent>
        </Tooltip>
      </div>

      {drafts.length > 0 && (
        <div className="px-3 pb-2">
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm" className="w-full justify-start">
                <FilePen /> Entwürfe ({drafts.length})
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-72">
              {drafts.map((d) => (
                <DropdownMenuItem key={d.id} onSelect={() => onOpenDraft(d)} className="flex flex-col items-start gap-0">
                  <span className="w-full truncate font-medium">{d.subject || "(kein Betreff)"}</span>
                  <span className="w-full truncate text-xs text-muted-foreground">
                    {d.to ? `An: ${d.to}` : "Noch kein Empfänger"} · {formatListDate(d.updatedAt)}
                  </span>
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      )}

      <ScrollArea className="min-h-0 flex-1">
        <nav className="space-y-4 px-2 pb-4">
          {accounts.map((account) => (
            <AccountSection
              key={account.id}
              account={account}
              folders={byAccount.get(account.id) ?? []}
              state={status[account.id]?.state}
              error={status[account.id]?.error ?? null}
              selectedFolderId={selectedFolderId}
              onSelectFolder={onSelectFolder}
            />
          ))}
          {accounts.length === 0 && (
            <div className="px-2 py-6 text-sm text-muted-foreground">
              Noch kein Konto eingerichtet.
              <Button variant="outline" className="mt-3 w-full" onClick={onAddAccount}>
                <Plus /> Konto hinzufügen
              </Button>
            </div>
          )}
        </nav>
      </ScrollArea>

      {progress && (
        <div className="border-t px-3 py-2 text-xs text-muted-foreground">
          Synchronisiere {progress.folder}: {progress.done}/{progress.total}
        </div>
      )}
      {outbox.length > 0 && (
        <Tooltip>
          <TooltipTrigger asChild>
            <div className="flex items-center gap-2 border-t px-3 py-2 text-xs text-amber-600 dark:text-amber-400">
              <Upload className="size-3.5" /> Postausgang: {outbox.length} nicht gesendet
            </div>
          </TooltipTrigger>
          <TooltipContent side="right" className="max-w-xs">
            {outbox.map((o) => o.error ?? "Wartet auf Versand").join("\n")} – wird automatisch erneut versucht.
          </TooltipContent>
        </Tooltip>
      )}
      <div className="flex gap-1 border-t p-2">
        <Button variant="ghost" size="sm" className="flex-1 justify-start" onClick={onAddAccount}>
          <Plus /> Konto
        </Button>
        <Button variant="ghost" size="sm" className="flex-1 justify-start" onClick={onOpenSettings}>
          <Settings /> Einstellungen
        </Button>
      </div>
    </div>
  );
}

interface AccountSectionProps {
  account: Account;
  folders: Folder[];
  state?: "syncing" | "idle" | "error";
  error: string | null;
  selectedFolderId: number | null;
  onSelectFolder: (folder: Folder) => void;
}

function AccountSection({ account, folders, state, error, selectedFolderId, onSelectFolder }: AccountSectionProps) {
  return (
    <div>
      <div className="flex items-center gap-1.5 px-2 pb-1 text-xs font-medium tracking-wide text-muted-foreground uppercase">
        <span className="truncate" title={account.email}>
          {account.displayName || account.email}
        </span>
        {state === "syncing" && <Loader2 className="size-3 shrink-0 animate-spin" aria-label="Synchronisiert" />}
        {state === "error" && (
          <Tooltip>
            <TooltipTrigger asChild>
              <AlertTriangle className="size-3.5 shrink-0 text-destructive" aria-label="Fehler" />
            </TooltipTrigger>
            <TooltipContent side="right" className="max-w-xs normal-case">
              {error}
            </TooltipContent>
          </Tooltip>
        )}
      </div>
      {folders.length === 0 && state !== "error" && (
        <div className="px-2 py-1 text-sm text-muted-foreground">Ordner werden geladen…</div>
      )}
      <ul>
        {folders.map((f) => {
          const Icon = (f.role && roleIcon[f.role]) || FolderIcon;
          const active = f.id === selectedFolderId;
          return (
            <li key={f.id}>
              <button
                type="button"
                disabled={!f.selectable}
                onClick={() => onSelectFolder(f)}
                style={{ paddingLeft: `${0.5 + folderDepth(f) * 0.9}rem` }}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md py-1.5 pr-2 text-left text-sm transition-colors",
                  "hover:bg-sidebar-accent hover:text-sidebar-accent-foreground disabled:opacity-50",
                  active && "bg-sidebar-accent font-medium text-sidebar-accent-foreground",
                )}
              >
                <Icon className="size-4 shrink-0 opacity-70" />
                <span className="flex-1 truncate">{leafName(f)}</span>
                {f.unreadCount > 0 && f.role !== "sent" && f.role !== "trash" && (
                  <span className="text-xs font-semibold tabular-nums">{f.unreadCount}</span>
                )}
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
