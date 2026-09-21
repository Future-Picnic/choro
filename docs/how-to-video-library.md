# Local video library

From the project root:

```sh
python3 docs/serve-video-library.py
```

Open the printed `http://127.0.0.1:8769/how-to-video-library.html#start` address. Keep the terminal running; Ctrl+C stops it. Use `--port 0` if that port is occupied. Restart after editing the HTML/manifest.

Click a completed lesson to preview its latest export, open the video separately, open its folder in Finder, or copy the local video link. Playback stops when the modal closes. The storyboard remains below the player; unfinished lessons have no video controls.

The completed videos are 4K HEVC. The 29 review drafts are 1080p H.264 with free local Kokoro AI narration and no avatar; they are not marked completed or approved. They cover Agents, Band, Tasks & Docs, Review & Ship, Run & Preview, Orbit, Mobile and Productivity. Each draft modal includes playback, its actual runtime, a separate video link, a Finder action, and the matching narration storyboard. Draft mappings and scripts live in `draft-video-files` and `draft-video-scripts`; all 14 existing completed mappings remain unchanged.

Of the 43 unfinished non-Design lessons in this production pass, 29 have playable drafts (about 38 minutes) and 14 still require recordings. On 2026-09-21 the standalone Band Stop/Resume lesson was removed and Create Specialists on Demand was added. Git Workflows now teaches a reusable dev-to-main merge workflow, not a CI lesson; its private Demo branch is prepared. Demo was rebuilt with the composer fix, but runtime retry is waiting on macOS Keychain approval. Earlier per-lesson notes retain the prior capture blockers, not confirmed defects in the rebuilt version. Six Design lessons remain excluded. A ready draft can still carry a specific review caveat: the shortcut-customization draft saves and restores a binding but has not demonstrated successful invocation of the custom shortcut.

Use Safari on this Mac for HEVC playback, or Finder → QuickTime if another browser lacks HEVC support. Copied links work only on this Mac while the launcher is running; they are not public sharing links.

The page's `completed-video-files` JSON contains the single shared mapping for all 14 completed exports. Meet Choro now links to the 1:46 philosophy-first revision, with landing-page-inspired diagrams before the real app appears. Its modal includes the matching ten-scene script; the earlier export remains preserved. The library also includes Projects Have a Home, First Agents Across Projects, and the refreshed shortcut-overlay versions of Plan, Dictation, and Quick Ask. Only Design remains unfinished in Getting Started. No copies or uploads are made. The default folder is `~/Movies/Choro Tutorials`; override it with `--library-root /path/to/library` if the library moves.

Opening the HTML directly still shows the storyboards and local video links. Direct-file playback depends on browser permissions. Finder access requires the launcher; another static HTTP server cannot read arbitrary local video files.

The launcher binds only to 127.0.0.1, serves only this page and allowlisted video/poster files, supports byte-range seeking, and requires a per-process token plus same-origin POST for Finder actions. It does not expose a general shell or file browser.

## Checks

`python3 -B docs/test_video_library.py` checks media ranges, scope restrictions, missing files and Finder routing with the OS command mocked (no Finder UI). Tiny fixtures remain in an isolated temporary directory.

`node docs/test-video-library.cjs` requires an available `jsdom` installation (or `NODE_PATH` pointing to one). It checks all manifest mappings and all 63 modals in local-server, direct-file and ordinary-HTTP modes. It does not establish browser codec support or visually test playback.

“Shape an Idea with Docs” is completed (1:25), directly before “Work from Context: Tasks and Docs.” Its modal includes the approved assistant/drafting storyboard, a preview, direct file link and folder control for the verified clear-controls 4K export. “Take Choro with You on Mobile” is also completed (0:50): genuine iOS Simulator pairing to Demo, a phone reply, and its matching Mac continuation/result. Its modal links to the verified clear-layout 4K export. Pairing credentials and internal review messages are excluded from the video.
