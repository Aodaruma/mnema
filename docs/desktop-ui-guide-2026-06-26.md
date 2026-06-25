# Mnema Desktop UI ガイド

作成日: 2026-06-26

このメモは、現在の desktop MVP にある各画面の役割を整理するためのもの。
UI の見直しやフィードバック時に「この画面は何を担当するべきか」を確認できるようにする。

## 全体像

Mnema は、思いついたタスクをまず Inbox に入れ、Today で今日やることへ落とし込み、Schedule で時間割として確認し、Projects で長期構造を管理する。

基本の流れ:

1. Inbox に未整理のタスクを入れる。
2. Today で今日の候補タスクから実行計画を作る。
3. Schedule で保存済みの予定ブロックを確認・調整する。
4. Projects でプロジェクト、リスト、マイルストーンを整理する。
5. Assistant / Activity / Settings は補助画面として使う。

## Top Bar

アプリ全体の状態と共通操作を置く場所。

現在の役割:

- Vault path の確認。
- 接続中 backend の確認。
- Light / Dark mode の切り替え。
- Refresh によるデータ再読み込み。

現状の制限:

- Vault の切り替えや DB 設定の変更は Settings 側で行う。
- Refresh は全画面の主要データを読み直すが、細かい同期状態まではまだ表示しない。

## Navigation

左側の画面切り替え。

現在の画面:

- Today
- Inbox
- Projects
- Schedule
- Assistant
- Activity
- Settings

現状の制限:

- バッジ、件数、未読、警告などの状態表示はまだない。

## Inbox

未整理のタスクを素早く入れる場所。

使う場面:

- 思いついたことを忘れないように一旦入れる。
- プロジェクトや日程が未確定のタスクを仮置きする。
- `tomorrow`、`/due YYYY-MM-DD`、`45m`、`1.5h` などを含む短い入力からタスクを作る。

主な操作:

- Quick capture: 短い文章からタスク名、期限、見積時間を解釈して追加する。
- Task title / Due / Estimate: 明示的に入力してタスクを追加する。
- 一覧上の Edit / Done / Delete: 既存タスクを編集、完了、削除する。

この画面に置かないもの:

- 今日の時間割作成。
- プロジェクト全体の進捗管理。
- 保存済みスケジュールの編集。

現状の制限:

- AI による自動分類はまだ本格実装していない。
- Project / List / Milestone への直接割り当て UI はまだない。

## Today

今日の実行計画を作る場所。

使う場面:

- 今日やる候補タスクを確認する。
- Plan で今日の予定案を作る。
- Save で予定案を ScheduleBlock として保存する。
- 予定が崩れた時に Repair from 以降を再提案する。

主な操作:

- Date: 対象日を切り替える。
- Plan: 対象日のタスクから予定案を作る。まだ保存はしない。
- Save: 表示中の予定案を proposed ScheduleBlock として保存する。
- Repair from: その時刻以降の予定を再計画する開始時刻。
- Repair: scheduled / done / active / locked block を固定し、残りを再提案する。
- Save repair: repair 結果を proposed ScheduleBlock として保存する。
- Tasks: 今日の判断材料になるタスク一覧を編集する。

この画面に置かないもの:

- 月全体の予定確認。
- プロジェクト横断の長期ロードマップ。
- AutomationLog の監査。

現状の制限:

- availability は現時点では既定の 9:00-17:00 を使う。
- Repair は proposed block の置き換えが中心で、差分レビュー UI はまだない。
- 外部カレンダー予定はまだ取り込んでいない。

## Schedule

保存済み ScheduleBlock を確認・調整する場所。

使う場面:

- Today で保存した予定案を時間割として見る。
- proposed block を scheduled / done / cancelled に変える。
- 開始・終了時刻を手動で調整する。
- 月間 Calendar で予定密度を俯瞰する。

表示モード:

- Day: 1日のタイムラインと block 操作パネル。
- Calendar: 月間カレンダー。日付セルを選ぶと対象日が切り替わる。

ScheduleBlock の状態:

- proposed: Mnema が提案した未確定の予定。
- scheduled: ユーザーが予定として採用したもの。
- active: 実行中扱い。
- done: 完了。
- missed: 未消化。
- cancelled: キャンセル。

この画面に置かないもの:

- 新規タスクの大量投入。
- プロジェクト構造の編集。
- AI との会話。

現状の制限:

- Calendar 画面から block の直接編集はまだできない。
- drag and drop での時間変更はまだない。
- 繰り返し予定、外部カレンダー、通知はまだない。

## Projects

長期的な構造を管理する場所。

使う場面:

- Project を作る。
- Project に List を追加する。
- Project に Milestone を追加する。
- Gantt でプロジェクト期間と選択中プロジェクトのマイルストーンを確認する。

表示モード:

- Details: Project 一覧、選択中 Project の List / Milestone。
- Gantt: Project の start/end を横棒として表示する。選択中 Project の Milestone はマーカーとして表示する。

この画面に置かないもの:

- 今日の実行順の決定。
- 1日の予定ブロック編集。
- Inbox の未整理タスク投入。

現状の制限:

- Project への Task 割り当て UI はまだない。
- Gantt から start/end や milestone date の直接編集はまだできない。
- 依存関係の線表示はまだない。

## Assistant

会話による補助操作の場所。

使う場面:

- 現在のタスクや予定を文脈にして相談する。
- LLM provider を設定している場合に、簡易的な助言を受ける。
- 短い入力からタスク作成を補助する。

主な操作:

- Send: 入力内容を Assistant に送る。
- Settings で LLM provider、URL、model を設定する。

この画面に置かないもの:

- すべての変更を無確認で自動実行すること。
- 詳細な ScheduleBlock 編集。

現状の制限:

- Assistant からの計画修正提案と承認 UI はまだない。
- LLM の出力は提案として扱う。予定変更の決定主体にはしない。

## Activity

自動化や AI が関与した変更の履歴を見る場所。

使う場面:

- タスク作成などの自動化ログを確認する。
- 後から「何が変更されたか」を追う。

主な操作:

- Refresh: 最新ログを読み直す。

この画面に置かないもの:

- 通常のタスク編集。
- 今日の計画生成。

現状の制限:

- undo / rollback はまだない。
- review queue として承認・却下する UI はまだない。

## Settings

アプリの接続先と実行設定を管理する場所。

使う場面:

- Vault path を変更する。
- SQLite / PostgreSQL backend を選ぶ。
- SQLite DB path や PostgreSQL URL を設定する。
- LLM provider と model を設定する。
- 設定を保存し、再接続する。

主な操作:

- Save settings: 設定ファイルへ保存する。
- Connect: 現在の設定で Vault / DB へ接続する。

現状の制限:

- 設定変更後の migration 状態や接続詳細の可視化はまだ少ない。
- PostgreSQL の自動セットアップはまだない。

## UI フィードバック時の観点

見るとよい観点:

- その画面で何をすればよいか一目で分かるか。
- 入力欄の意味、単位、必須/任意が分かるか。
- Today と Schedule の役割が混ざって見えないか。
- Inbox と Projects の役割が混ざって見えないか。
- Calendar / Gantt が「見るだけ」なのか「編集できる」のか誤解しないか。
- Assistant がどこまで実行してよいのか分かるか。

今後の UI 改善候補:

- 画面ごとの短い補助ラベル。
- Navigation の件数バッジ。
- Calendar / Gantt の直接編集。
- Schedule repair の差分プレビュー。
- Assistant の提案承認 UI。
- Activity の undo / review UI。
