import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { ArrowDown, ArrowUp, Plus, RotateCw, Trash2 } from "lucide-react";
import { Field } from "@/components/field";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { api, errorMessage, type Account, type Signature } from "@/lib/api";
import { keys, useAccounts, useSettings, useSignatures } from "@/lib/queries";
import { GmailClientForm } from "./GmailClientForm";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SettingsDialog({ open, onOpenChange }: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[85vh] max-w-2xl overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Einstellungen</DialogTitle>
        </DialogHeader>
        <Tabs defaultValue="accounts">
          <TabsList className="w-full">
            <TabsTrigger value="accounts">Konten</TabsTrigger>
            <TabsTrigger value="signatures">Signaturen</TabsTrigger>
            <TabsTrigger value="gmail">Gmail</TabsTrigger>
            <TabsTrigger value="sync">Synchronisation</TabsTrigger>
          </TabsList>
          <TabsContent value="accounts" className="space-y-3 pt-2">
            <AccountsTab />
          </TabsContent>
          <TabsContent value="signatures" className="pt-2">
            <SignaturesTab />
          </TabsContent>
          <TabsContent value="gmail" className="pt-2">
            <p className="mb-3 text-sm text-muted-foreground">
              OAuth-Client aus der Google Cloud Console (Typ „Desktop-App“). Das Secret wird im Schlüsselbund gespeichert.
            </p>
            <GmailClientForm />
          </TabsContent>
          <TabsContent value="sync" className="pt-2">
            <SyncTab />
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

function AccountsTab() {
  const qc = useQueryClient();
  const { data: accounts = [] } = useAccounts();
  const { data: signatures = [] } = useSignatures();
  if (accounts.length === 0) return <p className="text-sm text-muted-foreground">Keine Konten eingerichtet.</p>;

  const move = async (index: number, delta: number) => {
    const next = [...accounts];
    const [item] = next.splice(index, 1);
    next.splice(index + delta, 0, item);
    // Optimistic: the sidebar reorders immediately.
    qc.setQueryData(keys.accounts, next);
    try {
      await api.accountsReorder(next.map((a) => a.id));
    } catch (e) {
      toast.error(errorMessage(e));
    }
    qc.invalidateQueries({ queryKey: keys.accounts });
    qc.invalidateQueries({ queryKey: keys.folders });
  };

  return (
    <>
      <p className="text-xs text-muted-foreground">Mit den Pfeilen legen Sie die Reihenfolge der Konten in der Seitenleiste fest.</p>
      {accounts.map((a, i) => (
        <AccountEditor
          key={a.id}
          account={a}
          signatures={signatures}
          onMoveUp={i > 0 ? () => move(i, -1) : undefined}
          onMoveDown={i < accounts.length - 1 ? () => move(i, 1) : undefined}
        />
      ))}
    </>
  );
}

function AccountEditor({
  account,
  signatures,
  onMoveUp,
  onMoveDown,
}: {
  account: Account;
  signatures: Signature[];
  onMoveUp?: () => void;
  onMoveDown?: () => void;
}) {
  const qc = useQueryClient();
  const [name, setName] = useState(account.displayName);
  const [senderName, setSenderName] = useState(account.senderName);
  const [signatureId, setSignatureId] = useState<string>(account.signatureId ? String(account.signatureId) : "none");
  const [password, setPassword] = useState("");

  const refresh = () => {
    qc.invalidateQueries({ queryKey: keys.accounts });
    qc.invalidateQueries({ queryKey: keys.folders });
    qc.invalidateQueries({ queryKey: ["messages"] });
  };
  const run = async (p: Promise<unknown>, done: string) => {
    try {
      await p;
      toast.success(done);
      refresh();
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const remove = () => {
    if (!window.confirm(`Konto ${account.email} entfernen? Lokal gespeicherte Mails dieses Kontos werden gelöscht.`)) return;
    run(api.accountRemove(account.id), "Konto entfernt");
  };

  return (
    <div className="space-y-3 rounded-lg border p-3">
      <div className="flex items-center justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-medium">
            {account.displayName} <span className="font-normal text-muted-foreground">· {account.email}</span>
          </div>
          <div className="text-xs text-muted-foreground">
            {account.kind === "gmail" ? "Gmail (OAuth2)" : `IMAP ${account.imapHost}:${account.imapPort} · SMTP ${account.smtpHost}:${account.smtpPort}`}
          </div>
        </div>
        <div className="flex shrink-0 gap-1">
          <Button variant="ghost" size="icon-sm" aria-label="Nach oben" title="Nach oben" disabled={!onMoveUp} onClick={onMoveUp}>
            <ArrowUp />
          </Button>
          <Button variant="ghost" size="icon-sm" aria-label="Nach unten" title="Nach unten" disabled={!onMoveDown} onClick={onMoveDown}>
            <ArrowDown />
          </Button>
          <Button variant="ghost" size="icon-sm" aria-label="Neu verbinden" title="Neu verbinden" onClick={() => run(api.accountReconnect(account.id), "Verbindung wird neu aufgebaut")}>
            <RotateCw />
          </Button>
          <Button variant="ghost" size="icon-sm" aria-label="Konto entfernen" title="Konto entfernen" onClick={remove}>
            <Trash2 />
          </Button>
        </div>
      </div>
      <div className="grid grid-cols-2 gap-3">
        <Field label="Kontoname (nur in nuntii)">
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={account.email} />
        </Field>
        <Field label="Absendername (sehen Empfänger)">
          <Input value={senderName} onChange={(e) => setSenderName(e.target.value)} placeholder="leer = nur E-Mail-Adresse" />
        </Field>
        <Field label="Signatur">
          <Select value={signatureId} onValueChange={setSignatureId}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">Keine</SelectItem>
              {signatures.map((s) => (
                <SelectItem key={s.id} value={String(s.id)}>
                  {s.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      </div>
      <div className="flex flex-wrap items-end justify-end gap-2">
        {account.kind === "imap" && (
          <div className="flex flex-1 items-end gap-2">
            <Field label="Neues Passwort">
              <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
            </Field>
            <Button
              variant="outline"
              disabled={!password}
              onClick={() => run(api.accountUpdatePassword(account.id, password).then(() => setPassword("")), "Passwort aktualisiert")}
            >
              Passwort ändern
            </Button>
          </div>
        )}
        {account.kind === "gmail" && (
          <Button variant="outline" onClick={() => run(api.accountAddGmail(null), "Gmail neu autorisiert")}>
            Neu bei Google anmelden
          </Button>
        )}
        <Button
          onClick={() =>
            run(api.accountUpdate(account.id, name, senderName, signatureId === "none" ? null : Number(signatureId)), "Konto gespeichert")
          }
        >
          Speichern
        </Button>
      </div>
    </div>
  );
}

function SignaturesTab() {
  const qc = useQueryClient();
  const { data: signatures = [] } = useSignatures();
  const [editing, setEditing] = useState<{ id: number | null; name: string; body: string } | null>(null);

  const save = async () => {
    if (!editing) return;
    try {
      await api.signatureSave(editing.id, editing.name, editing.body);
      await qc.invalidateQueries({ queryKey: keys.signatures });
      setEditing(null);
      toast.success("Signatur gespeichert");
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const remove = async (id: number) => {
    await api.signatureDelete(id).catch((e) => toast.error(errorMessage(e)));
    qc.invalidateQueries({ queryKey: keys.signatures });
    qc.invalidateQueries({ queryKey: keys.accounts });
  };

  return (
    <div className="space-y-3">
      <ul className="space-y-1">
        {signatures.map((s) => (
          <li key={s.id} className="flex items-center gap-2 rounded-md border px-3 py-2 text-sm">
            <button type="button" className="flex-1 truncate text-left" onClick={() => setEditing({ id: s.id, name: s.name, body: s.bodyText })}>
              {s.name}
            </button>
            <Button variant="ghost" size="icon-xs" aria-label="Signatur löschen" onClick={() => remove(s.id)}>
              <Trash2 />
            </Button>
          </li>
        ))}
      </ul>
      {editing ? (
        <div className="space-y-2 rounded-lg border p-3">
          <Field label="Name">
            <Input value={editing.name} onChange={(e) => setEditing({ ...editing, name: e.target.value })} />
          </Field>
          <Field label="Text">
            <Textarea rows={5} value={editing.body} onChange={(e) => setEditing({ ...editing, body: e.target.value })} />
          </Field>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setEditing(null)}>
              Abbrechen
            </Button>
            <Button onClick={save}>Speichern</Button>
          </div>
        </div>
      ) : (
        <Button variant="outline" onClick={() => setEditing({ id: null, name: "", body: "" })}>
          <Plus /> Neue Signatur
        </Button>
      )}
      <p className="text-xs text-muted-foreground">Die Signatur wird dem Konto unter „Konten“ zugeordnet.</p>
    </div>
  );
}

function SyncTab() {
  const qc = useQueryClient();
  const { data: settings } = useSettings();
  const [days, setDays] = useState(90);
  useEffect(() => {
    if (settings) setDays(settings.syncWindowDays);
  }, [settings]);

  const save = async () => {
    try {
      await api.settingsSetSyncWindow(days);
      await qc.invalidateQueries({ queryKey: keys.settings });
      await api.syncNow(null);
      toast.success("Gespeichert – ältere Mails werden jetzt nachgeladen");
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  return (
    <div className="space-y-3">
      <Field label="Mails der letzten … Tage abgleichen">
        <Input type="number" min={1} max={3650} value={days} onChange={(e) => setDays(Number(e.target.value))} className="w-32" />
      </Field>
      <p className="text-xs text-muted-foreground">
        Neue Mails werden alle 2 Minuten abgerufen. Mails der letzten 30 Tage in Posteingang und Gesendet werden automatisch
        für das Offline-Lesen heruntergeladen, ältere beim Öffnen.
      </p>
      <Button onClick={save}>Speichern</Button>
    </div>
  );
}
