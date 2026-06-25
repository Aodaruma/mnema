# Mnema 残実装メモ

作成日: 2026-06-25

## 現在できること

- egui / eframe のデスクトップアプリとして起動できる。
- SQLite を既定 backend とし、PostgreSQL は任意 backend として残している。
- Vault path を指定して接続できる。
- Inbox / Home / Schedule / Assistant / Settings の基本画面がある。
- Projects 画面で Project / List / Milestone を追加・選択できる。
- Inbox からタスクを追加できる。
- Quick capture で `tomorrow`、`/due YYYY-MM-DD`、`45m`、`1.5h` などを解釈してタスク化できる。
- タスク一覧でステータス変更、タスク名・期限・見積の行内編集、削除ができる。
- タスク名はダブルクリックでその場編集できる。
- due date は `YYYY-MM-DD` / `YYYYMMDD` / `today` / `tomorrow` / `tue` などを解釈し、Home ではfloating popupの日付セレクターまたは候補リストから選べる。
- estimate は `30m` / `1h30m` / `01:30` / `01:30:00` / 分数を解釈し、Home ではfloating popupの固定値・加算ボタンからも選べる。
- タスク一覧のステータス変更は `Ctrl+Z` / `Ctrl+Shift+Z` で undo / redo できる。
- Home では完了済みタスクを三角トグルで表示でき、直近10件から追加読み込みできる。完了済みタスクのステータスも変更できる。
- Home ではタスク行を agenda へドラッグして、manual / locked な ScheduleBlock として保存できる。
- Home 表示時は現在時刻ライン付近へ自動スクロールできる。
- Home で plan を生成し、ScheduleBlock として自動保存できる。
- Home の plan は過去日や当日の現在時刻以前には配置しない。
- Home で scheduled / done / active / locked block を固定し、現在時刻以降の proposed block をRepairボタンまたは現在時刻ラインのアイコンから再提案・自動保存できる。
- Plan / Repair 結果は proposed / unscheduled / issue 件数と issue 内容を表示できる。
- Plan / Repair / Manual の block は agenda 上で色分けできる。
- Schedule は日別タイムラインとして表示できる。
- Schedule は月間 Calendar 表示へ切り替えられる。
- Projects は Project Gantt 表示へ切り替えられる。
- 保存済み ScheduleBlock を scheduled / done / cancelled に変更でき、開始/終了時刻も編集できる。
- Home の plan 保存時、既存 proposed blocks を置き換える前に確認できる。
- due date / estimate / schedule 時刻は専用 UI で編集できる。
- Settings で Vault path、SQLite path、PostgreSQL URL、timezone offset、テーマ、LLM 設定を保存できる。
- Assistant 画面でタスク作成と、設定済み LLM を使った簡易相談ができる。
- タスク作成は AutomationLog に `CREATE_TASK` として記録される。
- Activity 画面で AutomationLog を確認できる。
- Light / Dark mode を切り替えられる。
- Noto Sans JP を優先フォントとして読み込み、通常は wght=400、強調箇所は wght=700 を指定している。
- 左上ロゴには Montserrat を同梱して使う。
- Navigation、theme、edit、delete、repair などの UI アイコンには Apache License 2.0 の Google Material Icons を同梱して使う。
- muda によるネイティブメニューバーの足場がある。
- `scripts/dev-desktop.ps1` で GUI を起動できる。
- `scripts/package-desktop-windows.ps1` で Windows 向け簡易パッケージを作れる。

## P0: まず「アプリ単体で触れる」ために必要

- 完了:
  - due date / estimate を日付・数値入力に寄せた。
  - フォームの主要バリデーションメッセージを日本語化した。
  - `cargo run` 以外で触れる dev 起動スクリプトを追加した。
  - Windows 向けの簡易ビルド・配布手順を追加した。

## P1: Mnema らしさを出す中核機能

- 秘書チャット MVP
  - LLM を使った応答生成に接続する。完了。
  - 軽い計画修正をチャットから行う。
- AutomationLog
  - AI / 自動化が何を変更したかを確認できる UI。完了。
  - 将来的なレビュー・取り消しの土台にする。
- ジョブ基盤
  - Inbox 分類、週次レビュー、日次ふりかえりなどをバックグラウンドジョブとして扱う。

## P2: 統合案の後続拡張

- Project / Milestone / List の基本 UI。完了。
- Calendar / Gantt の基本表示。完了。
- 週次レビュー生成
- 外部カレンダー連携
- モバイル・Web クライアント再評価
- 同期方式の検討

## DB 方針

当面は SQLite を既定にする。ローカルアプリとしてすぐ触れることを優先するためである。

PostgreSQL は self-hosted / server 運用向けの任意 backend として残す。Dropbox Desktop / Google Drive Desktop などが SQLite ファイルをロックする懸念はあるため、将来的には UI から backend を切り替えられる状態を目指す。

## 次に着手する推奨順

1. Assistant からの計画修正提案と承認 UI。
2. Calendar / Gantt の編集操作、フィルタ、依存関係表示。
3. AutomationLog の review / undo UI。
4. 週次レビュー生成。
5. Windows インストーラー、署名、auto-update。
