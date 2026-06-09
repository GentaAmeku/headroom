**English** | [日本語](README.md)

# Headroom

A macOS **menu bar** app that shows your **AI coding tool usage quotas at a glance**. Click the icon to see how much of each tool's usage window you've consumed and when it resets — rendered as a native macOS menu (NSMenu).

> Headroom = the room you have left. Check your remaining quota before you hit a wall.

<p align="center">
  <img src="docs/screenshot.jpeg" alt="Headroom menu showing Claude, Cursor, and Codex usage" width="720">
</p>

## Supported tools

| Tool | Shows | Source (read-only) |
|------|-------|--------------------|
| **Claude** | 5-Hour & Weekly | macOS Keychain `Claude Code-credentials` → `api.anthropic.com/api/oauth/usage` |
| **Cursor** | Monthly (included) + On-Demand overage | `state.vscdb` (`cursorAuth/accessToken`) → `api2.cursor.sh` |
| **Codex** | 5-Hour & Weekly | `~/.codex/auth.json` → `chatgpt.com/backend-api/codex/usage` |

Each tool appears only if you're signed in to it; otherwise Headroom shows a gentle "not connected" note. Usage refreshes on launch, every 5 minutes, and via the **Refresh** menu item.

## Privacy & security

- **Read-only.** Headroom never refreshes tokens or writes to any credential store.
- **Local only.** Credentials never leave your machine; requests go directly to each tool's own API. No telemetry, no servers.

## Install

### Homebrew (recommended)

```sh
brew install --cask --no-quarantine GentaAmeku/tap/headroom
```

The app is not code-signed (it's a free, open-source utility), so `--no-quarantine` lets it open without a Gatekeeper prompt.

### Manual

1. Download `Headroom_<version>_universal.dmg` from [Releases](https://github.com/GentaAmeku/headroom/releases).
2. Drag **Headroom.app** into Applications.
3. On first launch, right-click the app → **Open** (once), or run:
   ```sh
   xattr -dr com.apple.quarantine /Applications/Headroom.app
   ```

Headroom registers itself as a **login item** so it starts automatically. Remove it anytime in System Settings → General → Login Items.

## Configuration

Cursor's "included" monthly budget varies by plan; Headroom defaults to **$20/month** and shows anything above it as a separate **On-Demand** line. Override the budget with either:

- Environment variable: `HEADROOM_CURSOR_BUDGET=50`
- or `~/.config/headroom/config.json`:
  ```json
  { "cursorMonthlyBudgetUsd": 50, "language": "ja" }
  ```

The **display language** follows your macOS locale (Japanese / English). To force it, set `"language"` to `"ja"` or `"en"` in the `config.json` above.

## Build from source

Requires [Rust](https://rustup.rs) and [Node.js](https://nodejs.org).

```sh
npm install
npm run tauri build -- --bundles dmg     # release .dmg
npm run tauri build -- --debug --bundles app   # quick debug build
```

The bundle lands in `src-tauri/target/<profile>/bundle/macos/Headroom.app`.

## Contributing & internals

Project conventions and architecture live in [`AGENTS.md`](AGENTS.md), with domain terms in [`CONTEXT.md`](CONTEXT.md), UI guidelines in [`design.md`](design.md), and decisions in [`docs/adr/`](docs/adr/).

## License

[MIT](LICENSE) © gameku
