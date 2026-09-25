import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { CheckCircle2, Loader2 } from "lucide-react";
import { Field } from "@/components/field";
import { Logo } from "@/components/logo";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api, errorMessage, type AccountConfig, type Security } from "@/lib/api";
import { keys, useSettings } from "@/lib/queries";
import { GmailClientForm } from "../settings/GmailClientForm";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const emptyImap: AccountConfig = {
  kind: "imap",
  displayName: "",
  senderName: "",
  email: "",
  username: "",
  imapHost: "",
  imapPort: 993,
  imapSecurity: "tls",
  smtpHost: "",
  smtpPort: 465,
  smtpSecurity: "tls",
};

export function AccountSetupDialog({ open, onOpenChange }: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl sm:max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Logo className="size-5" /> Konto hinzufügen
          </DialogTitle>
          <DialogDescription>Zugangsdaten werden ausschließlich im Schlüsselbund des Betriebssystems gespeichert.</DialogDescription>
        </DialogHeader>
        <Tabs defaultValue="imap">
          <TabsList className="w-full">
            <TabsTrigger value="imap">Synology / IMAP</TabsTrigger>
            <TabsTrigger value="gmail">Gmail</TabsTrigger>
          </TabsList>
          <TabsContent value="imap">
            <ImapForm onDone={() => onOpenChange(false)} />
          </TabsContent>
          <TabsContent value="gmail">
            <GmailForm onDone={() => onOpenChange(false)} />
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

function SecuritySelect({ value, onChange }: { value: Security; onChange: (v: Security) => void }) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v as Security)}>
      <SelectTrigger className="w-full">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="tls">SSL/TLS</SelectItem>
        <SelectItem value="starttls">STARTTLS</SelectItem>
      </SelectContent>
    </Select>
  );
}

function ImapForm({ onDone }: { onDone: () => void }) {
  const qc = useQueryClient();
  const [c, setC] = useState<AccountConfig>(emptyImap);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState<"test" | "add" | null>(null);
  const [tested, setTested] = useState(false);
  const set = (patch: Partial<AccountConfig>) => {
    setTested(false);
    setC((prev) => ({ ...prev, ...patch }));
  };

  const onEmail = (email: string) => {
    const domain = email.split("@")[1] ?? "";
    set({
      email,
      username: c.username && c.username !== c.email ? c.username : email,
      // Sensible guesses; Synology MailPlus usually runs on the NAS hostname.
      imapHost: c.imapHost || !domain ? c.imapHost : `mail.${domain}`,
      smtpHost: c.smtpHost || !domain ? c.smtpHost : `mail.${domain}`,
    });
  };

  const config = (): AccountConfig => ({ ...c, displayName: c.displayName.trim() || c.email });

  const test = async () => {
    setBusy("test");
    try {
      await api.accountTest(config(), password);
      setTested(true);
      toast.success("Verbindung erfolgreich");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const add = async () => {
    setBusy("add");
    try {
      await api.accountAddImap(config(), password);
      await qc.invalidateQueries({ queryKey: keys.accounts });
      toast.success("Konto hinzugefügt – Synchronisation läuft");
      setC(emptyImap);
      setPassword("");
      onDone();
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <form
      className="space-y-4 pt-2"
      onSubmit={(e) => {
        e.preventDefault();
        add();
      }}
    >
      <div className="grid grid-cols-2 gap-3">
        <Field label="E-Mail-Adresse">
          <Input type="email" required value={c.email} onChange={(e) => onEmail(e.target.value)} />
        </Field>
        <Field label="Absendername (sehen Empfänger)">
          <Input value={c.senderName} onChange={(e) => set({ senderName: e.target.value })} placeholder="Max Mustermann" />
        </Field>
        <Field label="Kontoname (nur in nuntii)">
          <Input value={c.displayName} onChange={(e) => set({ displayName: e.target.value })} placeholder="z. B. Praxis" />
        </Field>
        <div />
        <Field label="Benutzername">
          <Input required value={c.username} onChange={(e) => set({ username: e.target.value })} />
        </Field>
        <Field label="Passwort">
          <Input
            type="password"
            required
            value={password}
            onChange={(e) => {
              setTested(false);
              setPassword(e.target.value);
            }}
          />
        </Field>
      </div>
      <div className="grid grid-cols-[1fr_5.5rem_8rem] gap-3">
        <Field label="IMAP-Server">
          <Input required value={c.imapHost} onChange={(e) => set({ imapHost: e.target.value })} placeholder="nas.example.de" />
        </Field>
        <Field label="Port">
          <Input type="number" required value={c.imapPort} onChange={(e) => set({ imapPort: Number(e.target.value) })} />
        </Field>
        <Field label="Sicherheit">
          <SecuritySelect
            value={c.imapSecurity}
            onChange={(v) => set({ imapSecurity: v, imapPort: v === "tls" ? 993 : 143 })}
          />
        </Field>
        <Field label="SMTP-Server">
          <Input required value={c.smtpHost} onChange={(e) => set({ smtpHost: e.target.value })} placeholder="nas.example.de" />
        </Field>
        <Field label="Port">
          <Input type="number" required value={c.smtpPort} onChange={(e) => set({ smtpPort: Number(e.target.value) })} />
        </Field>
        <Field label="Sicherheit">
          <SecuritySelect
            value={c.smtpSecurity}
            onChange={(v) => set({ smtpSecurity: v, smtpPort: v === "tls" ? 465 : 587 })}
          />
        </Field>
      </div>
      <p className="text-xs text-muted-foreground">
        Nur verschlüsselte Verbindungen. Bei selbst signierten Zertifikaten des NAS muss dessen Zertifikat bzw. CA im
        Betriebssystem als vertrauenswürdig installiert sein.
      </p>
      <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" onClick={test} disabled={busy !== null || !password}>
          {busy === "test" ? <Loader2 className="animate-spin" /> : tested ? <CheckCircle2 className="text-green-600" /> : null}
          Verbindung testen
        </Button>
        <Button type="submit" disabled={busy !== null}>
          {busy === "add" && <Loader2 className="animate-spin" />} Hinzufügen
        </Button>
      </div>
    </form>
  );
}

function GmailForm({ onDone }: { onDone: () => void }) {
  const qc = useQueryClient();
  const { data: settings } = useSettings();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const configured = !!settings?.gmailClientId && settings.gmailHasSecret;

  const connect = async () => {
    setBusy(true);
    try {
      await api.accountAddGmail(name.trim() || null);
      await qc.invalidateQueries({ queryKey: keys.accounts });
      toast.success("Gmail verbunden – Synchronisation läuft");
      onDone();
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-4 pt-2">
      {!configured ? (
        <>
          <p className="text-sm text-muted-foreground">
            Für Gmail wird ein eigener OAuth-Client aus der Google Cloud Console benötigt (Typ „Desktop-App“, Gmail-API
            aktiviert, eigenes Konto als Testnutzer eingetragen).
          </p>
          <GmailClientForm />
        </>
      ) : (
        <>
          <Field label="Absendername (sehen Empfänger, optional)">
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Max Mustermann" />
          </Field>
          <p className="text-sm text-muted-foreground">
            Die Anmeldung öffnet sich im Browser. Nach der Freigabe kehrt nuntii automatisch zurück.
          </p>
          <div className="flex justify-end">
            <Button onClick={connect} disabled={busy}>
              {busy && <Loader2 className="animate-spin" />}
              {busy ? "Warte auf Anmeldung im Browser…" : "Mit Google anmelden"}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

