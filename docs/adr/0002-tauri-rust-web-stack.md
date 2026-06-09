# 技術スタックに Tauri (Rust コア + Web フロント) を採用

## Context

常駐メニューバーアプリで、(1) 軽さ・バッテリーが重要、(2) スクショ並みの作り込んだ UI、(3) 認証情報の読み取り・HTTP・OAuth トークン更新という機微な処理、(4) macOS 専用・個人/OSS、という要件。ユーザーは Web/React が主戦場、Rust 経験あり（`rust-htmx-dashboard`）、Swift は環境はあるが実務経験が薄い。

## Decision

Tauri v2 を採用。UI は Web フロント、機微なコア（認証情報・HTTP・トークン更新）は Rust で実装。メニューバー常駐は tray title（テキスト表示）＋ tray 直下のポップオーバー窓（`tauri-plugin-positioner`）＋ `ActivationPolicy::Accessory`（Dock 非表示）＋ `tauri-plugin-autostart`（ログイン時起動）で構成。

## Considered Options

- **Swift / SwiftUI** — 最もネイティブで NSStatusItem / NSPopover / Keychain / Swift Charts が純正。却下理由: Swift 実務経験が薄く学習コストが高い割に、個人ツールでネイティブ純正感の差を得るメリットが薄い。
- **Electron** — Web スキルが活きるが、常駐アプリで数百MB級は「効率を測る道具」として本末転倒。却下。

## Consequences

- メニューバーの色分けされた凝った表示はできない（tray はプレーンテキスト）。必要なら tray アイコンを動的に画像生成して差し替える方式になる。
- ポップオーバーは macOS 純正 NSPopover ではなく「直下に置いた枠なし窓」。見た目は近いが矢印付きの純正感は出ない。
- Keychain 等の OS 連携はプラグイン経由。
