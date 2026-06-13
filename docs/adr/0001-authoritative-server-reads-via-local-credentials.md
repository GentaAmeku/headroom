# 認証情報を使ったサーバー権威値の取得（ローカル実行・個人/少数配布）

## Context

このツールの核は各 AI ツールの **Usage Window の残量と Pace** を正確に把握すること。正確な絶対値、かつ複数デバイス横断の合計が要件。ローカルログ推定（案A）は (1) 別デバイスでの利用分を原理的に取得できず、(2) 各社が枠の上限を非公開のため近似（±10〜20%）に留まる。サーバーだけが全デバイス横断の真値を持つ。

## Decision

各ツールがローカルに保存した認証情報（例: `~/.claude/.credentials.json`, `~/.codex/auth.json`, Cursor の Cookie/トークン）を用い、各ツール**非公式**の usage エンドポイントからサーバー権威値を取得する（案B）。アプリは各ユーザーのマシン上で**ローカル実行**し、認証情報は端末外に出さない（中央サーバーに集約しない）。配布形態は GitHub 上の OSS + 少数の知人を想定。

## Considered Options

- **案A: ローカルログ推定** — 却下。マルチデバイス合計が取れず、上限非公開で近似値に留まる。トークン消費の履歴・モデル別内訳など補助情報の供給源としては引き続き有用なため、完全には捨てない。
- **案B: 認証API読み取り** — 採用。

## Consequences

- 各ツールごとに非公式APIの実装・OAuth等のトークン更新・各社の仕様変更追従が必要（もろさ・継続メンテ前提）。
- ToS グレーゾーンを各ユーザーが「自分のアカウント範囲」で負う。認証情報はアカウント全権を持つため取り扱いは読み取り目的でも厳重に。
- **採用前に各エンドポイントを spike（技術検証）して到達可能性を確認する。** 特に Copilot は個人の残量を返す到達可能なエンドポイントが存在しない可能性があり、MVP 対象から外れうる。

## Spike 結果 — Claude（2026-06-08 確認済み）

- **エンドポイント**: `GET https://api.anthropic.com/api/oauth/usage`
- **必須ヘッダ**: `Authorization: Bearer <accessToken>`, `anthropic-beta: oauth-2025-04-20`
- **認証情報のソースは macOS Keychain**（サービス名 `Claude Code-credentials`）**と `~/.claude/.credentials.json` の両方**。Claude Code のバージョン/操作によって、片方だけが更新されることがあるため、実装では両方を読み、`expiresAt` が未失効の候補のうち最も新しいものを使う。トークン失効時は `/v1/oauth/token` でリフレッシュ可能だが、refreshToken ローテーションに注意し、更新後は元のストアに書き戻す必要がある。
- **レスポンス**: `five_hour` / `seven_day` がそれぞれ `{ utilization: 0-100, resets_at: ISO8601 }` を返す（= Consumption と Reset）。加えてモデル別週次枠 `seven_day_opus` / `seven_day_sonnet`（該当プランのみ）、サブスク超過従量 `extra_usage` を含む。
- → Claude については B 案の実現性が**確定**。
