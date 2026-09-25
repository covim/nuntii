import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  keepPreviousData,
  QueryClient,
  useInfiniteQuery,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { api, type ListFilter, type AccountStatusEvent, type MailChangedEvent, type SyncProgressEvent } from "./api";

export const queryClient = new QueryClient({
  defaultOptions: {
    // Data is local (SQLite); the backend tells us via events when it changes.
    queries: { staleTime: Infinity, refetchOnWindowFocus: false, retry: false },
  },
});

export const PAGE_SIZE = 200;

export const keys = {
  accounts: ["accounts"] as const,
  folders: ["folders"] as const,
  messages: (folderId: number) => ["messages", folderId] as const,
  thread: (threadId: number) => ["thread", threadId] as const,
  message: (id: number, allowRemote: boolean) => ["message", id, allowRemote] as const,
  search: (q: string) => ["search", q] as const,
  settings: ["settings"] as const,
  signatures: ["signatures"] as const,
  outbox: ["outbox"] as const,
  drafts: ["drafts"] as const,
  accountStatus: ["accountStatus"] as const,
  syncProgress: ["syncProgress"] as const,
};

export const useAccounts = () => useQuery({ queryKey: keys.accounts, queryFn: api.accountsList });

export const useFolders = () => useQuery({ queryKey: keys.folders, queryFn: () => api.foldersList(null) });

/** `keepId`: the open message stays listed under a filter even after it was marked read. */
export function useMessages(folderId: number | null, filter: ListFilter, keepId: number | null) {
  const keep = filter === "all" ? null : keepId;
  return useInfiniteQuery({
    queryKey: [...keys.messages(folderId ?? -1), filter, keep],
    enabled: folderId != null,
    initialPageParam: 0,
    placeholderData: keepPreviousData,
    queryFn: ({ pageParam }) => api.messagesList(folderId!, pageParam, PAGE_SIZE, filter, keep),
    getNextPageParam: (last, pages) => (last.length < PAGE_SIZE ? undefined : pages.length * PAGE_SIZE),
  });
}

export const useThread = (threadId: number | null) =>
  useQuery({
    queryKey: keys.thread(threadId ?? -1),
    enabled: threadId != null,
    queryFn: () => api.threadGet(threadId!),
  });

export const useMessage = (id: number | null, allowRemote: boolean) =>
  useQuery({
    queryKey: keys.message(id ?? -1, allowRemote),
    enabled: id != null,
    queryFn: () => api.messageGet(id!, allowRemote),
    staleTime: 60_000,
  });

export const useSearch = (q: string) =>
  useQuery({
    queryKey: keys.search(q),
    enabled: q.trim().length > 0,
    queryFn: () => api.search(q, 200),
    staleTime: 0,
  });

export const useSettings = () => useQuery({ queryKey: keys.settings, queryFn: api.settingsGet });
export const useSignatures = () => useQuery({ queryKey: keys.signatures, queryFn: api.signaturesList });
export const useOutbox = () => useQuery({ queryKey: keys.outbox, queryFn: api.outboxList });
export const useDrafts = () => useQuery({ queryKey: keys.drafts, queryFn: api.draftsList });

export type StatusMap = Record<number, AccountStatusEvent>;
export const useAccountStatus = () =>
  useQuery<StatusMap>({ queryKey: keys.accountStatus, queryFn: () => ({}), initialData: {} });
export const useSyncProgress = () =>
  useQuery<SyncProgressEvent | null>({ queryKey: keys.syncProgress, queryFn: () => null, initialData: null });

/** Refreshes the local-data queries whenever the backend reports changes. */
export function useBackendEvents() {
  const qc = useQueryClient();
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const pending = new Set<number>();
    let all = false;

    const flush = () => {
      qc.invalidateQueries({ queryKey: keys.folders });
      qc.invalidateQueries({ queryKey: keys.outbox });
      qc.invalidateQueries({ queryKey: ["thread"] });
      qc.invalidateQueries({ queryKey: ["search"] });
      if (all) qc.invalidateQueries({ queryKey: ["messages"] });
      else pending.forEach((id) => qc.invalidateQueries({ queryKey: keys.messages(id) }));
      pending.clear();
      all = false;
    };

    const unlisteners = [
      listen<MailChangedEvent>("mail://changed", ({ payload }) => {
        if (payload.folderIds.length === 0) all = true;
        payload.folderIds.forEach((id) => pending.add(id));
        clearTimeout(timer);
        // Initial sync emits many events in a row; coalesce them.
        timer = setTimeout(flush, 250);
      }),
      listen<AccountStatusEvent>("account://status", ({ payload }) => {
        qc.setQueryData<StatusMap>(keys.accountStatus, (prev) => ({ ...prev, [payload.accountId]: payload }));
        if (payload.state !== "syncing") qc.setQueryData(keys.syncProgress, null);
      }),
      listen<SyncProgressEvent>("sync://progress", ({ payload }) => {
        qc.setQueryData(keys.syncProgress, payload.done >= payload.total ? null : payload);
      }),
    ];
    return () => {
      clearTimeout(timer);
      unlisteners.forEach((p) => p.then((un) => un()));
    };
  }, [qc]);
}
