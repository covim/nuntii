import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

interface HtmlBodyProps {
  document: string;
  onMailto: (address: string) => void;
}

/**
 * Renders sanitized mail HTML in a sandboxed iframe. `allow-scripts` is deliberately absent, so
 * no script can run inside; `allow-same-origin` only lets this component intercept link clicks
 * and measure the content height.
 */
export function HtmlBody({ document: doc, onMailto }: HtmlBodyProps) {
  const ref = useRef<HTMLIFrameElement>(null);
  const observer = useRef<ResizeObserver | null>(null);
  const [height, setHeight] = useState(200);

  const onLoad = useCallback(() => {
    const inner = ref.current?.contentDocument;
    if (!inner) return;

    // The iframe never scrolls vertically itself (see wrap_document); it grows to fit its content,
    // including a possible horizontal scrollbar for wide newsletters.
    const measure = () => {
      const root = inner.documentElement;
      const hScrollbar = (inner.defaultView?.innerHeight ?? root.clientHeight) - root.clientHeight;
      setHeight(Math.max(Math.ceil(root.scrollHeight + hScrollbar), 60));
    };
    measure();
    observer.current?.disconnect();
    observer.current = new ResizeObserver(measure);
    observer.current.observe(inner.body);
    inner.querySelectorAll("img").forEach((img) => img.addEventListener("load", measure));
    inner.fonts?.ready.then(measure);

    inner.addEventListener("click", (e) => {
      const a = (e.target as Element | null)?.closest("a");
      if (!a) return;
      e.preventDefault();
      const href = a.getAttribute("href") ?? "";
      if (href.toLowerCase().startsWith("mailto:")) {
        onMailto(decodeURIComponent(href.slice(7).split("?")[0]));
      } else if (/^https?:\/\//i.test(href)) {
        openUrl(href);
      }
    });
  }, [onMailto]);

  useEffect(() => () => observer.current?.disconnect(), []);

  return (
    <iframe
      ref={ref}
      title="Nachrichteninhalt"
      sandbox="allow-same-origin"
      srcDoc={doc}
      onLoad={onLoad}
      style={{ height }}
      className="w-full rounded-md border bg-white"
    />
  );
}
