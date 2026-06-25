# Mnema 残実装メモ

作成日: 2026-06-25

## 現在できること

- egui / eframe のデスクトップアプリとして起動できる。
- SQLite を既定 backend とし、PostgreSQL は任意 backend として残している。
- Vault path を指定して接続できる。
- Inbox / Today / Schedule / Assistant / Settings の基本画面がある。
- Inbox からタスクを追加できる。
- Quick capture で `tomorrow`、`/due YYYY-MM-DD`、`45m`、`1.5h` などを解釈してタスク化できる。
- タスク一覧で編集・完了・削除ができる。
- Today plan を生成し、ScheduleBlock として保存できる。
- Schedule は日別タイムラインとして表示できる。
- 保存済み ScheduleBlock を scheduled / done / cancelled に変更でき、開始/終了時刻も編集できる。
- Today plan 保存時、既存 proposed blocks を置き換える前に確認できる。
- Settings で Vault path、SQLite path、PostgreSQL URL、テーマ、LLM 設定を保存できる。
- Assistant 画面でタスク作成と簡易的な「次に何をするか」の相談ができる。
- タスク作成は AutomationLog に `CREATE_TASK` として記録される。
- Light / Dark mode を切り替えられる。
- Noto Sans JP を優先フォントとして読み込み、通常は wght=400、強調箇所は wght=700 を指定している。
- 左上ロゴには Montserrat を同梱して使う。
- muda によるネイティブメニューバーの足場がある。

## P0: まず「アプリ単体で触れる」ために必要

- 入力体験の改善
  - due date / estimate を日付ピッカーや数値入力に寄せる。
  - フォームのバリデーションメッセージを日本語化する。
- 配布・起動手順
  - `cargo run` 以外で触れる dev 起動スクリプト。
  - Windows 向けの簡易ビルド・配布手順。

## P1: Mnema らしさを出す中核機能

- 秘書チャット MVP
  - LLM を使った応答生成に接続する。
  - 軽い計画修正をチャットから行う。
- AutomationLog
  - AI / 自動化が何を変更したかを確認できる UI。
  - 将来的なレビュー・取り消しの土台にする。
- ジョブ基盤
  - Inbox 分類、週次レビュー、日次ふりかえりなどをバックグラウンドジョブとして扱う。

## P2: 統合案の後続拡張

- Project / Milestone / List の UI
- Gantt または Calendar 表示
- 週次レビュー生成
- 外部カレンダー連携
- モバイル・Web クライアント再評価
- 同期方式の検討

## DB 方針

当面は SQLite を既定にする。ローカルアプリとしてすぐ触れることを優先するためである。

PostgreSQL は self-hosted / server 運用向けの任意 backend として残す。Dropbox Desktop / Google Drive Desktop などが SQLite ファイルをロックする懸念はあるため、将来的には UI から backend を切り替えられる状態を目指す。

## 次に着手する推奨順

1. 日付ピッカー / 数値入力 / 日本語バリデーション。
2. AutomationLog の確認 UI。
3. Assistant を実 LLM クライアントに接続。
4. Project / Milestone / List UI。
5. Windows 向け dev 起動・配布手順。
