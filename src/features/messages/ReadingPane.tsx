import { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import {
  Download,
  Flag,
  FolderInput,
  ImageOff,
  Loader2,
  Mail,
  MailOpen,
  Forward,
  Reply,
  ReplyAll,
  Trash2,
} from "lucide-react";
import { Logo } from "@/components/logo";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { api, errorMessage, type MessageDetail, type MessageSummary } from "@/lib/api";
import { addressList, addressName, formatFullDate, formatListDate, formatSize, initials } from "@/lib/format";
import { useFolders, useMessage, useThread } from "@/lib/queries";
import { cn } from "@/lib/utils";
import { HtmlBody } from "./HtmlBody";

export type ComposeMode = "reply" | "replyAll" | "forward";

interface ReadingPaneProps {
  message: MessageSummary | null;
  onCompose: (mode: ComposeMode, source: MessageDetail) => void;
  onMailto: (address: string) => void;
  onDeleted: () => void;
}

function ToolButton({ label, onClick, children, disabled }: { label: string; onClick: () => void; children: React.ReactNode; disabled?: boolean }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="ghost" size="icon" aria-label={label} onClick={onClick} disabled={disabled}>
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

export function ReadingPane({ message, onCompose, onMailto, onDeleted }: ReadingPaneProps) {
  const { data: thread } = useThread(message?.threadId ?? null);
  const { data: folders = [] } = useFolders();
  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  useEffect(() => {
    setExpanded(new Set(message ? [message.id] : []));
  }, [message?.id]);

  // Mark as read when opened.
  useEffect(() => {
    if (message && !message.seen) {
      api.messagesSetFlag([message.id], "seen", true).catch((e) => toast.error(errorMessage(e)));
    }
  }, [message?.id]);

  const { data: detail } = useMessage(message?.id ?? null, false);

  if (!message) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
        <Logo className="size-16 opacity-20 grayscale" />
        <p className="text-sm">Keine Nachricht ausgewählt</p>
      </div>
    );
  }

  const run = (p: Promise<unknown>, done?: string) =>
    p.then(() => done && toast.success(done)).catch((e) => toast.error(errorMessage(e)));

  const moveTargets = folders.filter((f) => f.accountId === message.accountId && f.selectable && f.id !== message.folderId);
  const items = thread && thread.length > 0 ? thread : [message];

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-0.5 border-b px-2 py-1.5">
        <ToolButton label="Antworten (R)" disabled={!detail} onClick={() => detail && onCompose("reply", detail)}>
          <Reply />
        </ToolButton>
        <ToolButton label="Allen antworten (A)" disabled={!detail} onClick={() => detail && onCompose("replyAll", detail)}>
          <ReplyAll />
        </ToolButton>
        <ToolButton label="Weiterleiten (F)" disabled={!detail} onClick={() => detail && onCompose("forward", detail)}>
          <Forward />
        </ToolButton>
        <div className="mx-1 h-5 w-px bg-border" />
        <ToolButton
          label={message.seen ? "Als ungelesen markieren (U)" : "Als gelesen markieren (U)"}
          onClick={() => run(api.messagesSetFlag([message.id], "seen", !message.seen))}
        >
          {message.seen ? <Mail /> : <MailOpen />}
        </ToolButton>
        <ToolButton
          label={message.flagged ? "Markierung entfernen (S)" : "Markieren (S)"}
          onClick={() => run(api.messagesSetFlag([message.id], "flagged", !message.flagged))}
        >
          <Flag className={cn(message.flagged && "fill-orange-500 text-orange-500")} />
        </ToolButton>
        <DropdownMenu>
          <Tooltip>
            <TooltipTrigger asChild>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="icon" aria-label="Verschieben">
                  <FolderInput />
                </Button>
              </DropdownMenuTrigger>
            </TooltipTrigger>
            <TooltipContent>Verschieben</TooltipContent>
          </Tooltip>
          <DropdownMenuContent align="start" className="max-h-80 overflow-y-auto">
            {moveTargets.map((f) => (
              <DropdownMenuItem key={f.id} onSelect={() => run(api.messagesMove([message.id], f.id).then(onDeleted))}>
                {f.displayName}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <ToolButton label="Löschen (Entf)" onClick={() => run(api.messagesDelete([message.id]).then(onDeleted))}>
          <Trash2 />
        </ToolButton>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-4xl space-y-3 p-4">
          <h2 className="text-xl font-semibold break-words">{message.subject || "(kein Betreff)"}</h2>
          {items.map((m) =>
            expanded.has(m.id) ? (
              <ExpandedMessage key={m.id} summary={m} onMailto={onMailto} />
            ) : (
              <button
                key={m.id}
                type="button"
                onClick={() => setExpanded((s) => new Set(s).add(m.id))}
                className="flex w-full items-center gap-3 rounded-lg border px-4 py-2.5 text-left text-sm hover:bg-muted/60"
              >
                <span className="w-40 shrink-0 truncate font-medium">{addressName(m.from[0])}</span>
                <span className="flex-1 truncate text-muted-foreground">{m.snippet}</span>
                <span className="shrink-0 text-xs text-muted-foreground">{formatListDate(m.date)}</span>
              </button>
            ),
          )}
        </div>
      </div>
    </div>
  );
}

function ExpandedMessage({ summary, onMailto }: { summary: MessageSummary; onMailto: (a: string) => void }) {
  const [allowRemote, setAllowRemote] = useState(false);
  const { data: detail, error, isLoading } = useMessage(summary.id, allowRemote);

  const saveAttachment = async (idx: number, filename: string) => {
    const path = await save({ defaultPath: filename });
    if (!path) return;
    try {
      await api.attachmentSave(summary.id, idx, path);
      toast.success(`${filename} gespeichert`);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const sender = summary.from[0];
  const visibleAttachments = detail?.attachments.filter((a) => !(a.inline && a.contentId)) ?? [];

  return (
    <article className="rounded-lg border">
      <header className="flex items-start gap-3 border-b p-4">
        <Avatar className="size-9">
          <AvatarFallback>{initials(sender)}</AvatarFallback>
        </Avatar>
        <div className="min-w-0 flex-1 text-sm">
          <div className="flex items-baseline gap-2">
            <span className="truncate font-semibold">{addressName(sender)}</span>
            {sender && <span className="truncate text-xs text-muted-foreground">&lt;{sender.address}&gt;</span>}
          </div>
          <div className="truncate text-xs text-muted-foreground">An: {addressList(summary.to) || "–"}</div>
          {detail && detail.cc.length > 0 && (
            <div className="truncate text-xs text-muted-foreground">Cc: {addressList(detail.cc)}</div>
          )}
        </div>
        <time className="shrink-0 text-xs text-muted-foreground">{formatFullDate(summary.date)}</time>
      </header>

      <div className="p-4">
        {isLoading && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="size-4 animate-spin" /> Nachricht wird geladen…
          </div>
        )}
        {error && <div className="text-sm text-destructive">{errorMessage(error)}</div>}
        {detail && (
          <>
            {detail.blockedRemote > 0 && !detail.remoteAllowed && (
              <div className="mb-3 flex flex-wrap items-center gap-2 rounded-md bg-muted px-3 py-2 text-sm">
                <ImageOff className="size-4 text-muted-foreground" />
                <span className="flex-1">
                  {detail.blockedRemote} externe Inhalte blockiert (Schutz vor Tracking).
                </span>
                <Button size="sm" variant="outline" onClick={() => setAllowRemote(true)}>
                  Einmal laden
                </Button>
                {sender && (
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => api.senderTrust(sender.address).then(() => setAllowRemote(true))}
                  >
                    Immer von {sender.address}
                  </Button>
                )}
              </div>
            )}
            {detail.htmlDocument ? (
              <HtmlBody document={detail.htmlDocument} onMailto={onMailto} />
            ) : (
              <pre className="font-sans text-sm whitespace-pre-wrap break-words">{detail.text ?? ""}</pre>
            )}
            {visibleAttachments.length > 0 && (
              <div className="mt-4 flex flex-wrap gap-2">
                {visibleAttachments.map((a) => (
                  <button
                    key={a.id}
                    type="button"
                    onClick={() => saveAttachment(a.idx, a.filename)}
                    className="flex max-w-64 items-center gap-2 rounded-md border px-3 py-2 text-left text-sm hover:bg-muted/60"
                    title="Speichern unter…"
                  >
                    <Download className="size-4 shrink-0 text-muted-foreground" />
                    <span className="truncate">{a.filename}</span>
                    <span className="shrink-0 text-xs text-muted-foreground">{formatSize(a.size)}</span>
                  </button>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </article>
  );
}
