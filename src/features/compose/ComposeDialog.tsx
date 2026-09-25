import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { format } from "date-fns";
import { toast } from "sonner";
import { Loader2, Paperclip, Send, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import {
  api,
  errorMessage,
  type Account,
  type ComposeMode,
  type LocalDraft,
  type MessageDetail,
  type Signature,
} from "@/lib/api";
import { addressFull, addressName, formatFullDate, formatSize, normalizeSubject, splitRecipients } from "@/lib/format";
import { keys, useAccounts, useMessage, useSignatures } from "@/lib/queries";
import { escapeHtml, htmlToPlainText, RichEditor, textToHtml, useRichEditor } from "./RichEditor";

export interface ComposeRequest {
  mode: ComposeMode;
  source?: MessageDetail;
  to?: string;
  /** Resume a locally saved draft. */
  draft?: LocalDraft;
}

interface FormState {
  accountId: number;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  /** Initial editor content (HTML); the editor owns the text afterwards. */
  bodyHtml: string;
  showCc: boolean;
  forwardIdx: number[];
  paths: string[];
}

const AUTOSAVE_DELAY = 800;

function signatureBlock(account: Account | undefined, signatures: Signature[]): string {
  const sig = signatures.find((s) => s.id === account?.signatureId);
  return sig ? `<p></p><p>-- <br>${escapeHtml(sig.bodyText).replace(/\r?\n/g, "<br>")}</p>` : "";
}

function initialState(req: ComposeRequest, accounts: Account[], signatures: Signature[]): FormState {
  if (req.draft) {
    const d = req.draft;
    return {
      accountId: d.accountId,
      to: d.to,
      cc: d.cc,
      bcc: d.bcc,
      subject: d.subject,
      bodyHtml: d.bodyHtml,
      showCc: !!(d.cc || d.bcc),
      forwardIdx: d.forwardIdx,
      paths: d.attachmentPaths,
    };
  }

  const src = req.source;
  const accountId = src?.accountId ?? accounts[0]?.id ?? 0;
  const account = accounts.find((a) => a.id === accountId);
  // Empty first paragraph: the cursor starts above signature and quote.
  const sig = `<p></p>${signatureBlock(account, signatures)}`;
  const own = account?.email.toLowerCase();
  const base: FormState = {
    accountId,
    to: req.to ?? "",
    cc: "",
    bcc: "",
    subject: "",
    bodyHtml: sig,
    showCc: false,
    forwardIdx: [],
    paths: [],
  };
  if (!src) return base;

  const subject = normalizeSubject(src.subject);
  if (req.mode === "forward") {
    const header = [
      "-------- Weitergeleitete Nachricht --------",
      `Von: ${src.from.map(addressFull).join(", ")}`,
      `Datum: ${formatFullDate(src.date)}`,
      `Betreff: ${src.subject}`,
      `An: ${src.to.map(addressFull).join(", ")}`,
    ].join("\n");
    return {
      ...base,
      subject: `WG: ${subject}`,
      bodyHtml: `${sig}<p></p>${textToHtml(header)}${textToHtml(src.text ?? "")}`,
      forwardIdx: src.attachments.filter((a) => !(a.inline && a.contentId)).map((a) => a.idx),
    };
  }

  const to = src.from.map(addressFull);
  let cc: string[] = [];
  if (req.mode === "replyAll") {
    const seen = new Set([own, ...src.from.map((a) => a.address.toLowerCase())]);
    cc = [...src.to, ...src.cc]
      .filter((a) => {
        const key = a.address.toLowerCase();
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      })
      .map(addressFull);
  }
  const intro = `Am ${formatFullDate(src.date)} schrieb ${addressName(src.from[0])}:`;
  return {
    ...base,
    to: to.join(", "),
    cc: cc.join(", "),
    showCc: cc.length > 0,
    subject: `AW: ${subject}`,
    bodyHtml: `${sig}<p></p><p>${escapeHtml(intro)}</p><blockquote>${textToHtml((src.text ?? "").trimEnd())}</blockquote>`,
  };
}

const requestKeys = new WeakMap<ComposeRequest, number>();
let nextRequestKey = 0;
function requestKey(req: ComposeRequest): number {
  if (!requestKeys.has(req)) requestKeys.set(req, ++nextRequestKey);
  return requestKeys.get(req)!;
}

interface ComposeDialogProps {
  request: ComposeRequest | null;
  onClose: () => void;
}

export function ComposeDialog({ request, onClose }: ComposeDialogProps) {
  const { data: accounts = [] } = useAccounts();
  const { data: signatures = [] } = useSignatures();
  if (!request) return null;
  // Keyed by request: every compose request gets a fresh form and editor.
  return <ComposeForm key={requestKey(request)} request={request} onClose={onClose} accounts={accounts} signatures={signatures} />;
}

function ComposeForm({
  request,
  onClose,
  accounts,
  signatures,
}: ComposeDialogProps & { request: ComposeRequest; accounts: Account[]; signatures: Signature[] }) {
  const qc = useQueryClient();
  const [form, setForm] = useState<FormState>(() => initialState(request, accounts, signatures));
  const [sending, setSending] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(request.draft ? request.draft.updatedAt : null);
  const [bodyVersion, setBodyVersion] = useState(0);

  const sourceId = request.source?.id ?? request.draft?.sourceMessageId ?? null;
  // A resumed forward draft needs the original's attachment list again.
  const { data: resumedSource } = useMessage(!request.source && request.mode === "forward" ? sourceId : null, false);
  const source = request.source ?? resumedSource;

  // Autosave bookkeeping lives in refs so saves never race or resurrect a sent/discarded draft.
  const draftId = useRef<number | null>(request.draft?.id ?? null);
  const dirty = useRef(!!request.draft);
  const closed = useRef(false);
  const saving = useRef<Promise<void>>(Promise.resolve());
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const onEditorChange = useCallback(() => {
    dirty.current = true;
    setBodyVersion((v) => v + 1);
  }, []);
  const editor = useRichEditor(form.bodyHtml, !!form.to, onEditorChange);

  const forwardable = useMemo(() => source?.attachments.filter((a) => !(a.inline && a.contentId)) ?? [], [source]);

  const set = (patch: Partial<FormState>) => {
    dirty.current = true;
    setForm((f) => ({ ...f, ...patch }));
  };

  const formRef = useRef(form);
  formRef.current = form;

  const save = useCallback((): Promise<void> => {
    saving.current = saving.current.then(async () => {
      if (closed.current || !dirty.current || !editor) return;
      const f = formRef.current;
      try {
        draftId.current = await api.draftSave({
          id: draftId.current,
          accountId: f.accountId,
          mode: request.mode,
          sourceMessageId: sourceId,
          to: f.to,
          cc: f.cc,
          bcc: f.bcc,
          subject: f.subject,
          bodyHtml: editor.getHTML(),
          forwardIdx: f.forwardIdx,
          attachmentPaths: f.paths,
          updatedAt: 0,
        });
        setSavedAt(Math.floor(Date.now() / 1000));
        qc.invalidateQueries({ queryKey: keys.drafts });
      } catch (e) {
        toast.error(`Entwurf konnte nicht gespeichert werden: ${errorMessage(e)}`);
      }
    });
    return saving.current;
  }, [editor, qc, request.mode, sourceId]);

  // Debounced autosave after every change.
  useEffect(() => {
    if (!dirty.current) return;
    clearTimeout(timer.current);
    timer.current = setTimeout(save, AUTOSAVE_DELAY);
    return () => clearTimeout(timer.current);
  }, [form, bodyVersion, save]);

  /** Stops autosaving and waits for a save in flight. */
  const finish = async (flush: boolean) => {
    clearTimeout(timer.current);
    if (flush) await save();
    closed.current = true;
    await saving.current;
  };

  const close = async () => {
    const hadDraft = dirty.current;
    await finish(true);
    if (hadDraft && draftId.current != null) toast.success("Als Entwurf gespeichert");
    onClose();
  };

  const discard = async () => {
    if (dirty.current && !window.confirm("Entwurf verwerfen? Der Text geht verloren.")) return;
    await finish(false);
    if (draftId.current != null) {
      await api.draftDelete(draftId.current).catch((e) => toast.error(errorMessage(e)));
      qc.invalidateQueries({ queryKey: keys.drafts });
    }
    onClose();
  };

  const pickFiles = async () => {
    const picked = await open({ multiple: true, directory: false });
    if (!picked) return;
    const list = Array.isArray(picked) ? picked : [picked];
    set({ paths: [...form.paths, ...list] });
  };

  const send = async () => {
    if (!editor || sending) return;
    setSending(true);
    // Save the latest state first, so a failed validation still leaves an up-to-date draft.
    await finish(true);
    const html = editor.getHTML();
    try {
      const result = await api.composeSend({
        accountId: form.accountId,
        to: splitRecipients(form.to),
        cc: splitRecipients(form.cc),
        bcc: splitRecipients(form.bcc),
        subject: form.subject,
        bodyText: htmlToPlainText(html),
        bodyHtml: html,
        replyToMessageId: request.mode === "reply" || request.mode === "replyAll" ? sourceId : null,
        forwardMessageId: request.mode === "forward" ? sourceId : null,
        forwardAttachmentIdx: request.mode === "forward" ? form.forwardIdx : [],
        attachmentPaths: form.paths,
        draftId: draftId.current,
      });
      qc.invalidateQueries({ queryKey: keys.drafts });
      if (result.sent) {
        toast.success("Nachricht gesendet");
      } else {
        toast.warning(`Versand fehlgeschlagen, Nachricht liegt im Postausgang: ${result.error}`);
      }
      onClose();
    } catch (e) {
      // Not sent (e.g. invalid recipient): keep editing, autosave resumes.
      closed.current = false;
      toast.error(errorMessage(e));
    } finally {
      setSending(false);
    }
  };

  const title = request.mode === "forward" ? "Weiterleiten" : request.mode === "new" ? "Neue Nachricht" : "Antworten";

  return (
    <Dialog open>
      <DialogContent
        showCloseButton={false}
        // Modal: only the explicit buttons close the window, never a stray click or Esc.
        onInteractOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => e.preventDefault()}
        className="flex h-[85vh] max-w-4xl flex-col gap-3 sm:max-w-4xl"
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            send();
          }
        }}
      >
        <div className="flex items-center justify-between gap-2">
          <DialogTitle>{title}</DialogTitle>
          <Button variant="ghost" size="icon-sm" aria-label="Schließen (Entwurf wird gespeichert)" title="Schließen – Entwurf bleibt erhalten" onClick={close} disabled={sending}>
            <X />
          </Button>
        </div>

        <div className="grid grid-cols-[4.5rem_1fr] items-center gap-x-2 gap-y-2 text-sm">
          <Label>Von</Label>
          <Select value={String(form.accountId)} onValueChange={(v) => set({ accountId: Number(v) })}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {accounts.map((a) => (
                <SelectItem key={a.id} value={String(a.id)}>
                  {a.displayName} &lt;{a.email}&gt;
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Label htmlFor="compose-to">An</Label>
          <div className="flex gap-2">
            <Input id="compose-to" autoFocus={!form.to} value={form.to} onChange={(e) => set({ to: e.target.value })} />
            {!form.showCc && (
              <Button variant="ghost" size="sm" onClick={() => set({ showCc: true })}>
                Cc/Bcc
              </Button>
            )}
          </div>
          {form.showCc && (
            <>
              <Label htmlFor="compose-cc">Cc</Label>
              <Input id="compose-cc" value={form.cc} onChange={(e) => set({ cc: e.target.value })} />
              <Label htmlFor="compose-bcc">Bcc</Label>
              <Input id="compose-bcc" value={form.bcc} onChange={(e) => set({ bcc: e.target.value })} />
            </>
          )}
          <Label htmlFor="compose-subject">Betreff</Label>
          <Input id="compose-subject" value={form.subject} onChange={(e) => set({ subject: e.target.value })} />
        </div>

        <RichEditor editor={editor} className="flex-1" />

        {(forwardable.length > 0 || form.paths.length > 0) && (
          <div className="flex flex-wrap gap-2">
            {forwardable.map((a) => (
              <label key={a.idx} className="flex items-center gap-2 rounded-md border px-2 py-1 text-xs">
                <Checkbox
                  checked={form.forwardIdx.includes(a.idx)}
                  onCheckedChange={(c) =>
                    set({ forwardIdx: c ? [...form.forwardIdx, a.idx] : form.forwardIdx.filter((i) => i !== a.idx) })
                  }
                />
                {a.filename} <span className="text-muted-foreground">{formatSize(a.size)}</span>
              </label>
            ))}
            {form.paths.map((p) => (
              <span key={p} className="flex items-center gap-1 rounded-md border px-2 py-1 text-xs">
                <Paperclip className="size-3" /> {p.split(/[\\/]/).pop()}
                <button type="button" aria-label="Anhang entfernen" onClick={() => set({ paths: form.paths.filter((x) => x !== p) })}>
                  <X className="size-3" />
                </button>
              </span>
            ))}
          </div>
        )}

        <div className="flex items-center gap-2">
          <Button onClick={send} disabled={sending}>
            {sending ? <Loader2 className="animate-spin" /> : <Send />} Senden
          </Button>
          <Button variant="ghost" onClick={pickFiles} disabled={sending}>
            <Paperclip /> Anhang
          </Button>
          <Button variant="ghost" onClick={discard} disabled={sending} className="text-muted-foreground">
            <Trash2 /> Verwerfen
          </Button>
          <span className="ml-auto text-xs text-muted-foreground">
            {savedAt ? `Entwurf gespeichert ${format(new Date(savedAt * 1000), "HH:mm")} · ` : ""}Strg+Enter zum Senden
          </span>
        </div>
      </DialogContent>
    </Dialog>
  );
}
