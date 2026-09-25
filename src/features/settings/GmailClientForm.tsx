import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Field } from "@/components/field";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api, errorMessage } from "@/lib/api";
import { keys, useSettings } from "@/lib/queries";

/** Google OAuth client (ID in the DB, secret in the OS keychain). */
export function GmailClientForm() {
  const qc = useQueryClient();
  const { data: settings } = useSettings();
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");

  useEffect(() => setClientId(settings?.gmailClientId ?? ""), [settings?.gmailClientId]);

  const save = async () => {
    try {
      await api.settingsSetGmail(clientId, secret);
      setSecret("");
      await qc.invalidateQueries({ queryKey: keys.settings });
      toast.success("Google-OAuth-Client gespeichert");
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  return (
    <form
      className="space-y-3"
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <Field label="Client-ID">
        <Input required value={clientId} onChange={(e) => setClientId(e.target.value)} placeholder="…apps.googleusercontent.com" />
      </Field>
      <Field label={settings?.gmailHasSecret ? "Client-Secret (gespeichert – leer lassen zum Beibehalten)" : "Client-Secret"}>
        <Input type="password" required={!settings?.gmailHasSecret} value={secret} onChange={(e) => setSecret(e.target.value)} />
      </Field>
      <div className="flex justify-end">
        <Button type="submit">Speichern</Button>
      </div>
    </form>
  );
}
