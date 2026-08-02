# Choro Project Preview

This branch keeps one native `WKWebView` inside Choro and exposes it as a
collapsible, full-height **Preview** panel beside the Code and Agent workspaces.
The renderer is global, while preview identity is project-scoped: changing
agent chats does not reload or disconnect the current page.

## User flow

- Press **Preview** beside the project Run scripts control.
- Ask an agent to open a static page such as `index.html`; `preview_open`
  accepts project-relative HTML paths and loads them directly without a server.
- Choro detects HTTP(S) URLs printed by live project terminals and adds them to
  the service picker automatically.
- Agents can also call `preview_open` with a running URL. Each file or URL is
  stored once for the project, opens the panel, and becomes available to every
  chat.
- Use **Review**, then click one exact DOM element or drag an arbitrary image
  area. Both gestures open the inline comment composer in the page.
- The submitted crop and metadata are sent to whichever project agent is
  selected when Send is pressed. Selecting another chat never creates another
  WebView or another copy of the preview.

## Lifecycle

- Preview records and their service picker survive app restarts.
- Only the selected URL is rendered. Other services are lightweight records.
- Leaving Code/Agents, closing Preview, opening a modal, or entering Settings
  removes or hides the native child surface so it cannot cover GPUI controls.
- The WebView is created lazily and shares the existing `WebPreviewHost` used by
  docs, URL references, and agent visualizations.

Project Preview is Choro's single visual-review workflow. It does not require a
second application or a separate chat connection.
