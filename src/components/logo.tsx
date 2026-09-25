import { cn } from "@/lib/utils";

/** nuntii app logo (same artwork as src-tauri/app-icon.svg). */
export function Logo({ className }: { className?: string }) {
  return <img src="/logo.svg" alt="nuntii" draggable={false} className={cn("size-6 select-none", className)} />;
}
