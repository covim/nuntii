import { EditorContent, useEditor, useEditorState, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { TextStyleKit } from "@tiptap/extension-text-style";
import TextAlign from "@tiptap/extension-text-align";
import {
  AlignCenter,
  AlignLeft,
  AlignRight,
  Bold,
  Highlighter,
  Italic,
  Link as LinkIcon,
  List,
  ListOrdered,
  Quote,
  Redo2,
  RemoveFormatting,
  Strikethrough,
  Underline,
  Undo2,
} from "lucide-react";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

const FONTS = ["Arial", "Calibri", "Georgia", "Tahoma", "Times New Roman", "Trebuchet MS", "Verdana", "Courier New"];
const SIZES = ["10px", "12px", "14px", "16px", "18px", "24px", "32px"];
const DEFAULT = "default";

export function useRichEditor(initialHtml: string, autofocus: boolean, onChange?: () => void) {
  return useEditor({
    onUpdate: () => onChange?.(),
    extensions: [
      StarterKit.configure({
        heading: false,
        codeBlock: false,
        code: false,
        horizontalRule: false,
        link: { openOnClick: false, autolink: true, defaultProtocol: "https" },
      }),
      TextStyleKit.configure({ lineHeight: false }),
      TextAlign.configure({ types: ["paragraph"] }),
    ],
    content: initialHtml,
    autofocus: autofocus ? "start" : false,
    editorProps: { attributes: { class: "rich-editor-content", "aria-label": "Nachricht" } },
  });
}

function ToolButton({
  label,
  active,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={label}
          aria-pressed={active}
          // Keep the editor selection when clicking toolbar buttons.
          onMouseDown={(e) => e.preventDefault()}
          onClick={onClick}
          className={cn(
            "inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground [&_svg]:size-4",
            active && "bg-muted text-foreground",
          )}
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function ColorButton({ label, value, onChange, children }: { label: string; value: string; onChange: (c: string) => void; children: React.ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <label
          className="relative inline-flex size-7 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground [&_svg]:size-4"
          aria-label={label}
        >
          {children}
          <span className="absolute right-1 bottom-0.5 left-1 h-0.5 rounded" style={{ background: value }} />
          <input type="color" className="sr-only" value={value} onChange={(e) => onChange(e.target.value)} />
        </label>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

const Divider = () => <div className="mx-1 h-5 w-px bg-border" />;

function Toolbar({ editor }: { editor: Editor }) {
  const s = useEditorState({
    editor,
    selector: ({ editor: e }) => ({
      bold: e.isActive("bold"),
      italic: e.isActive("italic"),
      underline: e.isActive("underline"),
      strike: e.isActive("strike"),
      bullet: e.isActive("bulletList"),
      ordered: e.isActive("orderedList"),
      quote: e.isActive("blockquote"),
      link: e.isActive("link"),
      left: e.isActive({ textAlign: "left" }),
      center: e.isActive({ textAlign: "center" }),
      right: e.isActive({ textAlign: "right" }),
      font: (e.getAttributes("textStyle").fontFamily as string | undefined) ?? DEFAULT,
      size: (e.getAttributes("textStyle").fontSize as string | undefined) ?? DEFAULT,
      color: (e.getAttributes("textStyle").color as string | undefined) ?? "#000000",
      highlight: (e.getAttributes("textStyle").backgroundColor as string | undefined) ?? "#ffff00",
      canUndo: e.can().undo(),
      canRedo: e.can().redo(),
    }),
  });
  const chain = () => editor.chain().focus();

  const setLink = () => {
    const previous = editor.getAttributes("link").href as string | undefined;
    const url = window.prompt("Link-Adresse", previous ?? "https://");
    if (url === null) return;
    if (url.trim() === "" || url === "https://") chain().extendMarkRange("link").unsetLink().run();
    else chain().extendMarkRange("link").setLink({ href: url.trim() }).run();
  };

  return (
    <div className="flex flex-wrap items-center gap-0.5 border-b px-1.5 py-1">
      <Select
        value={FONTS.includes(s.font) ? s.font : DEFAULT}
        onValueChange={(v) => (v === DEFAULT ? chain().unsetFontFamily().run() : chain().setFontFamily(v).run())}
      >
        <SelectTrigger size="sm" className="h-7 w-36 text-xs" aria-label="Schriftart">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={DEFAULT}>Standardschrift</SelectItem>
          {FONTS.map((f) => (
            <SelectItem key={f} value={f} style={{ fontFamily: f }}>
              {f}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select
        value={SIZES.includes(s.size) ? s.size : DEFAULT}
        onValueChange={(v) => (v === DEFAULT ? chain().unsetFontSize().run() : chain().setFontSize(v).run())}
      >
        <SelectTrigger size="sm" className="h-7 w-24 text-xs" aria-label="Schriftgröße">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={DEFAULT}>Normal</SelectItem>
          {SIZES.map((sz) => (
            <SelectItem key={sz} value={sz}>
              {parseInt(sz, 10)} px
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Divider />
      <ToolButton label="Fett (Strg+B)" active={s.bold} onClick={() => chain().toggleBold().run()}>
        <Bold />
      </ToolButton>
      <ToolButton label="Kursiv (Strg+I)" active={s.italic} onClick={() => chain().toggleItalic().run()}>
        <Italic />
      </ToolButton>
      <ToolButton label="Unterstrichen (Strg+U)" active={s.underline} onClick={() => chain().toggleUnderline().run()}>
        <Underline />
      </ToolButton>
      <ToolButton label="Durchgestrichen" active={s.strike} onClick={() => chain().toggleStrike().run()}>
        <Strikethrough />
      </ToolButton>
      <ColorButton label="Schriftfarbe" value={s.color} onChange={(c) => chain().setColor(c).run()}>
        <span className="text-sm font-semibold">A</span>
      </ColorButton>
      <ColorButton label="Hervorheben" value={s.highlight} onChange={(c) => chain().setBackgroundColor(c).run()}>
        <Highlighter />
      </ColorButton>
      <Divider />
      <ToolButton label="Linksbündig" active={s.left} onClick={() => chain().setTextAlign("left").run()}>
        <AlignLeft />
      </ToolButton>
      <ToolButton label="Zentriert" active={s.center} onClick={() => chain().setTextAlign("center").run()}>
        <AlignCenter />
      </ToolButton>
      <ToolButton label="Rechtsbündig" active={s.right} onClick={() => chain().setTextAlign("right").run()}>
        <AlignRight />
      </ToolButton>
      <Divider />
      <ToolButton label="Aufzählung" active={s.bullet} onClick={() => chain().toggleBulletList().run()}>
        <List />
      </ToolButton>
      <ToolButton label="Nummerierung" active={s.ordered} onClick={() => chain().toggleOrderedList().run()}>
        <ListOrdered />
      </ToolButton>
      <ToolButton label="Zitat" active={s.quote} onClick={() => chain().toggleBlockquote().run()}>
        <Quote />
      </ToolButton>
      <ToolButton label="Link" active={s.link} onClick={setLink}>
        <LinkIcon />
      </ToolButton>
      <ToolButton label="Formatierung entfernen" onClick={() => chain().unsetAllMarks().unsetTextAlign().run()}>
        <RemoveFormatting />
      </ToolButton>
      <Divider />
      <ToolButton label="Rückgängig (Strg+Z)" onClick={() => chain().undo().run()}>
        <Undo2 className={cn(!s.canUndo && "opacity-40")} />
      </ToolButton>
      <ToolButton label="Wiederholen (Strg+Y)" onClick={() => chain().redo().run()}>
        <Redo2 className={cn(!s.canRedo && "opacity-40")} />
      </ToolButton>
    </div>
  );
}

export function RichEditor({ editor, className }: { editor: Editor | null; className?: string }) {
  if (!editor) return null;
  return (
    <div className={cn("flex min-h-0 flex-col overflow-hidden rounded-md border", className)}>
      <Toolbar editor={editor} />
      <EditorContent editor={editor} className="rich-editor min-h-0 flex-1 cursor-text overflow-y-auto" onClick={() => editor.commands.focus()} />
    </div>
  );
}

// ---- HTML helpers ---------------------------------------------------------------------------

export function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/** Plain text → paragraphs, keeping line breaks. */
export function textToHtml(text: string): string {
  return text
    .split(/\r?\n\r?\n/)
    .map((para) => `<p>${escapeHtml(para).replace(/\r?\n/g, "<br>")}</p>`)
    .join("");
}

/** Plain-text alternative for the HTML mail: quotes become "> " lines, lists get markers. */
export function htmlToPlainText(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const walk = (node: Node): string => {
    if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? "";
    if (!(node instanceof HTMLElement)) return "";
    const inner = () => Array.from(node.childNodes).map(walk).join("");
    switch (node.tagName) {
      case "BR":
        return "\n";
      case "P":
      case "DIV":
        return `${inner()}\n\n`;
      case "LI": {
        const ordered = node.parentElement?.tagName === "OL";
        const idx = Array.from(node.parentElement?.children ?? []).indexOf(node) + 1;
        return `${ordered ? `${idx}.` : "-"} ${inner().trim()}\n`;
      }
      case "UL":
      case "OL":
        return `${inner()}\n`;
      case "BLOCKQUOTE":
        return (
          inner()
            .trimEnd()
            .split("\n")
            .map((l) => (l ? `> ${l}` : ">"))
            .join("\n") + "\n\n"
        );
      case "A": {
        const text = inner();
        const href = node.getAttribute("href");
        return href && href !== text ? `${text} <${href}>` : text;
      }
      default:
        return inner();
    }
  };
  return walk(doc.body).replace(/\n{3,}/g, "\n\n").trim() + "\n";
}
