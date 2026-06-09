# AGENTS.md — Headroom

サブスク契約した AI コーディングツール（Claude Code / Cursor / Codex）の **利用枠の残量** を一目で把握する macOS メニューバー常駐アプリ。

## まず読む

| ドキュメント | 何が書いてあるか |
|---|---|
| [`CONTEXT.md`](./CONTEXT.md) | ドメイン用語集。**コード/会話ではここの用語を使う**（Tool / Usage Window / Consumption / Reset / Collector / Snapshot / On-Demand） |
| [`design.md`](./design.md) | UI デザインガイドライン（採用=案A）。**色・タイポ・余白はここのトークンを厳守** |
| [`docs/adr/`](./docs/adr/) | 後戻りしにくい決定の記録（0001〜0004） |
| [`docs/design/mockup.html`](./docs/design/mockup.html) | 採用UIの実物モック |

## プロダクトの核

- 見せるのは **Consumption（利用枠の消費率 %）と Reset（リセット時刻）だけ**。Pace・コストは MVP 対象外。
- MVP 対象 Tool: **Claude → Cursor → Codex** の順（Copilot は対象外）。
- メニューバーは**アイコンのみ**。クリックで**ネイティブメニュー（NSMenu）**が開き、各 Tool の Usage Window を項目として並べる（UI は OS 標準に委ねる。ADR-0005）。Tool 見出し（アイコン付き）はクリックで対応アプリを起動。

## アーキテクチャ

```
[Collector(Claude)] ─┐
[Collector(Cursor)] ─┼→ Snapshot(Usage Window[]) ─→ ネイティブメニュー(NSMenu)に描画
[Collector(Codex)]  ─┘     (起動時＋5分間隔＋「更新」)
```

- **スタック**: Tauri v2（Rust コア ＋ Web フロント）。tray ＋ 直下ポップオーバー ＋ `ActivationPolicy::Accessory`（Dock 非表示）＋ ログイン時起動。
- **Collector**: Tool ごとのアダプタ。認証情報取得 → usage エンドポイント → 正規化 Snapshot を返す。機微な処理は Rust 側。
- **取得タイミング**: 起動時＋**5分間隔**の軽い更新＋「更新」項目で手動取得（ADR-0005、429 回避のため低頻度）。MVP は**ローカルDBなし**。NSMenu の生成・差し替えはメインスレッドで行う。

## 絶対に守るルール（ガードレール）

1. **資格情報は読み取り専用**（ADR-0004）。トークンの自前リフレッシュ・資格情報ストア（Keychain 等）への**書き込みは禁止**。失効/失敗は赤系で明示し、直近値を stale として残す。
2. **ローカル実行・認証情報は端末外に出さない**（ADR-0001）。中央サーバーに集約しない。
3. **Usage Window は可変リスト＋パーセント正規化**（ADR-0003）。通貨/回数ベースは Collector 内で `used÷limit×100` に変換。固定スキーマ（5h/週次決め打ち）にしない。
4. **UI はネイティブメニュー（NSMenu）**。自作 Web UI を足さず OS 標準に委ねる（`design.md` / ADR-0005）。情報行は無効（グレー）、行動項目（見出し＝アプリ起動・更新・Quit）のみ有効。
5. ドメイン語は `CONTEXT.md` に合わせる。

## 開発

- Claude の usage 取得（実証済み）: **`~/.claude/.credentials.json` の `claudeAiOauth.accessToken`** を優先（旧来の Keychain `Claude Code-credentials` は更新されず**失効**していることがあるためフォールバック扱い）→ `GET https://api.anthropic.com/api/oauth/usage`（ヘッダ `Authorization: Bearer`, `anthropic-version: 2023-06-01`, `anthropic-beta: oauth-2025-04-20`）→ `five_hour` / `seven_day` の `{ utilization, resets_at }`。`expiresAt`(epoch ms) で**失効チェック**し、失効時は取得せず案内（**自前リフレッシュはしない**＝ADR-0004）。詳細は ADR-0001。
  - 注意: 失効した oauth トークンでこの endpoint を叩くと **429（rate_limit_error）が返る**（401 ではない）。「レート制限」と誤認しないこと。
- レンダリング対象は macOS WKWebView（近年の WebKit）。モダン CSS（`light-dark()` / `color-mix()` / `corner-shape`）をフォールバックなしで使ってよい。
- **多言語（i18n）**: UI 文字列は `tr(ja, en)` ヘルパで日本語/英語を出し分ける。言語は OS ロケール（`sys-locale`、`ja*`→日本語/それ以外→英語）で自動判定。`~/.config/headroom/config.json` の `"language": "ja"|"en"` で上書き可。新規の表示文字列は必ず `tr()`（または `match lang()`）でラップすること。

## ステータス

**Claude / Cursor / Codex 実装済み**（Tauri ＋ ネイティブメニュー）。アプリ名は **Headroom**。MVP の Collector は一通り完了。
- Claude: `~/.claude/.credentials.json`（`claudeAiOauth.accessToken`、無ければ Keychain `Claude Code-credentials` にフォールバック）→ `GET https://api.anthropic.com/api/oauth/usage`（`anthropic-version` ＋ `anthropic-beta: oauth-2025-04-20`）→ `five_hour` / `seven_day`。`expiresAt` で失効チェック。
- Cursor: `state.vscdb`（`~/Library/Application Support/Cursor/User/globalStorage/`）の `cursorAuth/accessToken` を sqlite3 で読む → `POST https://api2.cursor.sh/aiserver.v1.DashboardService/GetAggregatedUsageEvents`（Bearer, body `{}`）→ `totalCostCents`（当月の実利用額）。**Cursor API は上限を返さない**（Enterprise は全イベント `INCLUDED_IN_BUSINESS`／usage-based $0／`GetHardLimit`=`noUsageBasedAllowed`）ため、含まれる枠の上限はユーザー設定の定数 `CURSOR_INCLUDED_LIMIT_CENTS`（既定 $20）で表現し、2 Window に分ける: **Monthly**=`min(実利用額,上限)÷上限×100`（% 正規化, ADR-0003）／**On-Demand**=上限超過分を金額表示（CONTEXT.md の On-Demand、超過時のみ）。リセットは `GET https://api2.cursor.sh/auth/usage` の `startOfMonth`＋30日。
- Codex: `~/.codex/auth.json`（`tokens.access_token` / `account_id`）→ `GET https://chatgpt.com/backend-api/codex/usage`（`ChatGPT-Account-ID` ＋ `originator: codex_cli_rs`）→ `rate_limit.primary_window`(5h) / `secondary_window`(週次)。
- デバッグビルド: `npm run tauri build -- --debug --bundles app` → `src-tauri/target/debug/bundle/macos/Headroom.app`。
- Tool 見出しアイコン: 生成したブランド色の円（`brand_dot`、`design.md` §3）。実ロゴ PNG は解像度が不十分だったため不採用。
- トレイ（メニューバー）アイコン: 生成した「丸角の枠＋H」テンプレート画像（`menubar_icon`、`icon_as_template`）。`src/` の Web 資産は未使用（ADR-0005）。
