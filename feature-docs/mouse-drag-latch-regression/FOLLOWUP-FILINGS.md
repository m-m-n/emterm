# Follow-up filings (Notion)

このフィーチャの run で残った項目は、すでに Notion タスクとして起票済み。
**再起票しないこと。**

起票日時: 2026-09-21T01:15+09:00
起票元: em-workflow `mouse-drag-latch-regression` run（`--batch --once --pr`）の retrospect 完了時
起票経路: `notion-task-dispatch` の `task create` CLI（プロジェクト relation 解決済み）

| 由来 | タイトル | 種別 / 優先度 | Notion |
| --- | --- | --- | --- |
| review round 1 finding `a3855d779d6ae931`（medium / comprehensive / unresolved） | ボタン/ホイール経路の呼び出し側が live held 値を渡すことを固定するテストが無い (AC-3 の半分が未達) | 改善 / 中 | https://app.notion.com/p/live-held-AC-3-3e13509ec8ee8104be38d4b9553db3ed |
| batch-audit record `develop.scope-reconfirm-implement-entry` の `follow_up` | フォーカス喪失後に release が届かない経路は実在するか (残存 ghost drag の扱いの製品判断) | 調査 / — | https://app.notion.com/p/release-ghost-drag-3e13509ec8ee81d39de3ebc7a9c7a31b |
| batch-audit record `verify.manual-item-unexecutable` / `verify.not_executed_items` の TS-M1 | mouse-drag-latch-regression の TS-M1 (実機手動確認) を実施する | — / — | https://app.notion.com/p/mouse-drag-latch-regression-TS-M1-3e13509ec8ee8148a719e1c1a62d37ba |
| review round 1 findings `c48b3bbd49f304fa` / `5c6f95cc82a7beda`（medium / spec / unresolved） | TS-1 / TS-2 の doc コメントが mechanism ラベルを 2 つ挙げている (AC-6 / FR6 違反) | 改善 / 低 | https://app.notion.com/p/TS-1-TS-2-doc-mechanism-2-AC-6-FR6-3e13509ec8ee81bda658dff4d27ffa69 |

いずれもステータスは既定の「未着手」。Notion UI で「着手可能」に上げるとディスパッチが始まる。
