import {
  StrictMode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createRoot } from "react-dom/client";
import {
  BlockNoteSchema,
  defaultInlineContentSpecs,
  filterSuggestionItems,
  type PartialBlock,
} from "@blocknote/core";
import { BlockNoteView } from "@blocknote/mantine";
import {
  createReactInlineContentSpec,
  type DefaultReactSuggestionItem,
  SuggestionMenuController,
  useCreateBlockNote,
} from "@blocknote/react";
import "@mantine/core/styles.css";
import "@blocknote/mantine/style.css";
import "./style.css";

type ChoroTheme = {
  background: string;
  foreground: string;
  surface: string;
  muted: string;
  border: string;
  accent: string;
  danger: string;
  dark: boolean;
};

type ChoroDocument = {
  version: 1;
  format: "blocknote";
  title: string;
  blocks: PartialBlock[];
};

type MentionSource = {
  label: string;
  target: string;
  detail: string;
  kind: "file" | "asset";
  badge: string;
  previewUrl?: string;
};

type ChoroBootstrap = {
  path: string;
  document: ChoroDocument;
  files: MentionSource[];
  assets: MentionSource[];
  theme: ChoroTheme;
};

type HostApi = {
  loadDocument: (document: ChoroDocument) => void;
  setSources: (files: MentionSource[], assets: MentionSource[]) => void;
  setTheme: (theme: ChoroTheme) => void;
  resolveUpload: (requestId: string, url: string) => void;
  rejectUpload: (requestId: string, message: string) => void;
};

declare global {
  interface Window {
    ipc?: { postMessage: (message: string) => void };
    __CHORO_BOOTSTRAP__?: ChoroBootstrap;
    choroEditor?: HostApi;
  }
}

const Mention = createReactInlineContentSpec(
  {
    type: "mention",
    propSchema: {
      label: { default: "Unknown" },
      target: { default: "" },
      kind: { default: "file" },
    },
    content: "none",
  },
  {
    render: ({ inlineContent }) => {
      const prefix = inlineContent.props.kind === "asset" ? "@@" : "@";
      return (
        <span
          className={`choro-mention choro-mention-${inlineContent.props.kind}`}
          data-choro-target={inlineContent.props.target}
          title={inlineContent.props.target}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() =>
            postToHost({
              type: "openReference",
              path: bootstrap.path,
              target: inlineContent.props.target,
            })
          }
        >
          {prefix}{inlineContent.props.label}
        </span>
      );
    },
  },
);

const schema = BlockNoteSchema.create({
  inlineContentSpecs: {
    ...defaultInlineContentSpecs,
    mention: Mention,
  },
});

const defaultDocument: ChoroDocument = {
  version: 1,
  format: "blocknote",
  title: "Product Spec",
  blocks: [
    { type: "heading", props: { level: 1 }, content: "Product Spec" },
    { type: "heading", props: { level: 2 }, content: "Goal" },
    { type: "paragraph", content: "" },
    { type: "heading", props: { level: 2 }, content: "Context" },
    { type: "paragraph", content: "" },
    { type: "heading", props: { level: 2 }, content: "Requirements" },
    { type: "bulletListItem", content: "" },
    { type: "heading", props: { level: 2 }, content: "Open Questions" },
    { type: "bulletListItem", content: "" },
    { type: "heading", props: { level: 2 }, content: "Implementation Plan" },
    { type: "numberedListItem", content: "" },
    { type: "heading", props: { level: 2 }, content: "Acceptance Criteria" },
    { type: "checkListItem", content: "" },
  ],
};

const defaultTheme: ChoroTheme = {
  background: "#1f222b",
  foreground: "#e5e9f3",
  surface: "#252935",
  muted: "#959db0",
  border: "#363c4b",
  accent: "#9f98e8",
  danger: "#df7078",
  dark: true,
};

const bootstrap: ChoroBootstrap = window.__CHORO_BOOTSTRAP__ ?? {
  path: "preview.choro",
  document: defaultDocument,
  files: [
    {
      label: "README.md",
      target: "ref:file:README.md",
      detail: "README.md",
      kind: "file",
      badge: "File",
    },
  ],
  assets: [
    {
      label: "Product mockup",
      target: "ref:design:preview",
      detail: "Preview asset",
      kind: "asset",
      badge: "Image",
    },
  ],
  theme: defaultTheme,
};

const pendingUploads = new Map<
  string,
  { resolve: (url: string) => void; reject: (error: Error) => void }
>();

const macNavigationControls = new Set(["\u001c", "\u001d", "\u001e", "\u001f"]);

function installMacNavigationControlFix() {
  const beforeInput = (event: InputEvent) => {
    if (event.inputType !== "insertText" || !event.data) {
      return;
    }
    if (![...event.data].some((character) => macNavigationControls.has(character))) {
      return;
    }
    // WKWebView also reports an insertText control character after the real
    // Arrow* keydown. BlockNote already consumed that keydown, so only suppress
    // the bogus text insertion here; replaying it would move twice.
    event.preventDefault();
    event.stopPropagation();
  };
  document.addEventListener("beforeinput", beforeInput as EventListener, true);
  return () => {
    document.removeEventListener("beforeinput", beforeInput as EventListener, true);
  };
}

function postToHost(message: unknown) {
  window.ipc?.postMessage(JSON.stringify(message));
}

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("Could not read file"));
    reader.readAsDataURL(file);
  });
}

async function uploadFile(file: File): Promise<string> {
  if (file.size > 25 * 1024 * 1024) {
    throw new Error("Files larger than 25 MB are not supported.");
  }
  if (!window.ipc) {
    return URL.createObjectURL(file);
  }
  const requestId = crypto.randomUUID();
  const dataUrl = await readFileAsDataUrl(file);
  return new Promise((resolve, reject) => {
    pendingUploads.set(requestId, { resolve, reject });
    postToHost({
      type: "upload",
      requestId,
      name: file.name,
      mime: file.type,
      dataUrl,
    });
  });
}

function applyTheme(theme: ChoroTheme) {
  const root = document.documentElement;
  root.style.setProperty("--choro-bg", theme.background);
  root.style.setProperty("--choro-fg", theme.foreground);
  root.style.setProperty("--choro-surface", theme.surface);
  root.style.setProperty("--choro-muted", theme.muted);
  root.style.setProperty("--choro-border", theme.border);
  root.style.setProperty("--choro-accent", theme.accent);
  root.style.setProperty("--choro-danger", theme.danger);
  root.style.colorScheme = theme.dark ? "dark" : "light";
}

function App() {
  const [files, setFiles] = useState(bootstrap.files);
  const [assets, setAssets] = useState(bootstrap.assets);
  const [theme, setTheme] = useState(bootstrap.theme);
  const suppressChanges = useRef(false);
  const documentTitle = useRef(bootstrap.document.title);
  const pendingChange = useRef(false);
  const changeTimer = useRef<number | null>(null);
  const editor = useCreateBlockNote({
    schema,
    initialContent:
      bootstrap.document.blocks.length > 0
        ? bootstrap.document.blocks
        : defaultDocument.blocks,
    uploadFile,
  });

  const flushChanges = useCallback(() => {
    if (changeTimer.current !== null) {
      window.clearTimeout(changeTimer.current);
      changeTimer.current = null;
    }
    if (!pendingChange.current || suppressChanges.current) {
      return;
    }
    pendingChange.current = false;
    postToHost({
      type: "change",
      path: bootstrap.path,
      document: {
        version: 1,
        format: "blocknote",
        title: documentTitle.current,
        // The custom mention schema widens BlockNote's concrete Block type;
        // its persisted JSON remains compatible with PartialBlock on reload.
        blocks: editor.document as unknown as PartialBlock[],
      } satisfies ChoroDocument,
    });
  }, [editor]);

  const scheduleChange = useCallback(() => {
    pendingChange.current = true;
    if (changeTimer.current === null) {
      changeTimer.current = window.setTimeout(flushChanges, 120);
    }
  }, [flushChanges]);

  useEffect(() => {
    const removeMacNavigationFix = installMacNavigationControlFix();
    const flushBeforeLosingFocus = () => flushChanges();
    window.choroEditor = {
      loadDocument: (document) => {
        // Deliver locally typed content before accepting an external assistant
        // revision so the native side can merge or surface a real conflict.
        flushChanges();
        documentTitle.current = document.title;
        suppressChanges.current = true;
        try {
          editor.replaceBlocks(
            editor.document,
            document.blocks.length > 0 ? document.blocks : defaultDocument.blocks,
          );
        } finally {
          queueMicrotask(() => {
            suppressChanges.current = false;
          });
        }
      },
      setSources: (nextFiles, nextAssets) => {
        setFiles(nextFiles);
        setAssets(nextAssets);
      },
      setTheme: (nextTheme) => {
        setTheme(nextTheme);
        applyTheme(nextTheme);
      },
      resolveUpload: (requestId, url) => {
        const pending = pendingUploads.get(requestId);
        pendingUploads.delete(requestId);
        pending?.resolve(url);
      },
      rejectUpload: (requestId, message) => {
        const pending = pendingUploads.get(requestId);
        pendingUploads.delete(requestId);
        pending?.reject(new Error(message));
      },
    };
    applyTheme(bootstrap.theme);
    window.addEventListener("blur", flushBeforeLosingFocus);
    window.addEventListener("pagehide", flushBeforeLosingFocus);
    postToHost({ type: "ready", path: bootstrap.path });
    return () => {
      flushChanges();
      removeMacNavigationFix();
      window.removeEventListener("blur", flushBeforeLosingFocus);
      window.removeEventListener("pagehide", flushBeforeLosingFocus);
      for (const pending of pendingUploads.values()) {
        pending.reject(new Error("The document editor was closed during upload."));
      }
      pendingUploads.clear();
      delete window.choroEditor;
    };
  }, [editor, flushChanges]);

  const fileItems = useMemo(
    () =>
      files.map<DefaultReactSuggestionItem>((source) => ({
        title: source.label,
        subtext: source.detail,
        badge: source.badge,
        aliases: [source.target, source.detail],
        group: "Files",
        onItemClick: () => {
          editor.insertInlineContent([
            {
              type: "mention",
              props: {
                label: source.label,
                target: source.target,
                kind: source.kind,
              },
            },
            " ",
          ]);
        },
      })),
    [editor, files],
  );

  const assetItems = useMemo(
    () =>
      assets.map<DefaultReactSuggestionItem>((source) => ({
        title: source.label,
        badge: source.badge,
        aliases: [source.target, source.detail],
        group: "Assets",
        onItemClick: () => {
          if (source.previewUrl) {
            const currentBlock = editor.getTextCursorPosition().block;
            const image: PartialBlock = {
              type: "image",
              props: {
                url: source.previewUrl,
                name: source.label,
                caption: source.label,
                showPreview: true,
              },
            };
            if (Array.isArray(currentBlock.content) && currentBlock.content.length === 0) {
              editor.updateBlock(currentBlock, image);
            } else {
              editor.insertBlocks([image], currentBlock, "after");
            }
            return;
          }
          editor.insertInlineContent([
            {
              type: "mention",
              props: {
                label: source.label,
                target: source.target,
                kind: source.kind,
              },
            },
            " ",
          ]);
        },
      })),
    [assets, editor],
  );

  return (
    <main className="choro-editor-shell">
      <BlockNoteView
        className="choro-editor"
        editor={editor}
        theme={theme.dark ? "dark" : "light"}
        onChange={() => {
          if (suppressChanges.current) {
            return;
          }
          scheduleChange();
        }}
      >
        {/* Register the longer trigger first so the second @ upgrades the
            active file picker to the asset picker instead of reopening @. */}
        <SuggestionMenuController
          triggerCharacter="@@"
          getItems={async (query) =>
            filterSuggestionItems(assetItems, query).slice(0, 12)
          }
        />
        <SuggestionMenuController
          triggerCharacter="@"
          getItems={async (query) =>
            filterSuggestionItems(fileItems, query).slice(0, 12)
          }
        />
      </BlockNoteView>
    </main>
  );
}

applyTheme(bootstrap.theme);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
