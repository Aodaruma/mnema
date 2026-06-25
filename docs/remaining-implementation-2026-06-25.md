# Mnema 残実装メモ

作成日: 2026-06-25

## 現在できること

- egui / eframe のデスクトップアプリとして起動できる。
- SQLite を既定 backend とし、PostgreSQL は任意 backend として残している。
- Vault path を指定して接続できる。
- Inbox / Today / Schedule / Settings の基本画面がある。
- Inbox からタスクを追加できる。
- タスク一覧で編集・完了・削除ができる。
- Today plan を生成し、ScheduleBlock として保存できる。
- 保存済み ScheduleBlock を scheduled / done / cancelled に変更できる。
- Light / Dark mode を切り替えられる。
- Noto Sans JP を優先フォントとして読み込み、通常は wght=400、強調箇所は wght=700 を指定している。
- muda によるネイティブメニューバーの足場がある。

## P0: まず「アプリ単体で触れる」ために必要

- Vault 初期化 UX の改善
  - 初回起動時に既定 Vault をわかりやすく作る。
  - Settings で現在の backend / SQLite path / PostgreSQL URL を確認・変更できるようにする。
- 設定永続化
  - Vault path、テーマ、backend 選択をファイルまたは DB に保存する。
  - 環境変数だけに依存しない設定 UI に寄せる。
- Schedule 画面の操作拡張
  - 保存済みブロックの時刻・長さの編集。
  - 予定の再生成時に既存の scheduled / done を壊さない確認 UI。
  - proposed / scheduled / done / cancelled の表示をより判別しやすくする。
- 入力体験の改善
  - due date / estimate を空欄許可しつつ、日付ピッカーや数値入力に寄せる。
  - フォームのバリデーションメッセージを日本語化する。
- 配布・起動手順
  - `cargo run` 以外で触れる dev 起動スクリプト。
  - Windows 向けの簡易ビルド・配布手順。

## P1: Mnema らしさを出す中核機能

- 自然言語入力
  - 入力文から title / due / estimate を推定する。
  - LLM が無い場合も通常入力として動く fallback を持つ。
- 秘書チャット MVP
  - タスク追加、今日やることの相談、軽い計画修正をチャットから行う。
  - 変更は AutomationLog に記録する。
- AutomationLog
  - AI / 自動化が何を変更したかを確認できる履歴。
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

1. Settings の設定永続化と Vault 初期化 UX。
2. ScheduleBlock の時刻編集と再生成時の確認 UI。
3. 自然言語入力と LLM 設定 UI。
4. 秘書チャット MVP と AutomationLog。
