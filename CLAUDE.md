# CLAUDE.md

このプロジェクトの規約は [`AGENTS.md`](./AGENTS.md) に集約しています。**作業前に必ず `AGENTS.md` を読み、そこから参照される `CONTEXT.md`（用語）・`design.md`（UI）・`docs/adr/`（決定）に従ってください。**

特に重要なガードレール（詳細は AGENTS.md / ADR）:

1. 資格情報は**読み取り専用**。トークンのリフレッシュや Keychain 等への書き込みは禁止（ADR-0004）。
2. ローカル実行のみ。認証情報を端末外に出さない（ADR-0001）。
3. Usage Window は可変リスト＋パーセント正規化（ADR-0003）。
4. UI はネイティブメニュー（NSMenu）。自作 Web UI を足さず OS 標準に委ねる（`design.md` / ADR-0005）。
