# Mnema Desktop UI ガイド

作成日: 2026-06-26

このメモは、現在の desktop MVP にある各画面の役割を整理するためのもの。
UI の見直しやフィードバック時に「この画面は何を担当するべきか」を確認できるようにする。

## 全体像

Mnema は、思いついたタスクをまず Inbox に入れ、Home で今日やることへ落とし込み、Schedule で時間割として確認し、Projects で長期構造を管理する。

基本の流れ:

1. Inbox に未整理のタスクを入れる。
2. Home で今日の候補タスクから実行計画を作る。
3. Schedule で保存済みの予定ブロックを確認・調整する。
4. Projects でプロジェクト、リスト、マイルストーンを整理する。
5. Assistant / Activity / Settings は補助画面として使う。

## Top Bar

アプリ全体の状態と共通操作を置く場所。

現在の役割:

- 接続中 backend の確認。
- Light / Dark mode の切り替え。Material Icons の light / dark アイコンを使う。
- Refresh によるデータ再読み込み。Material Icons の refresh アイコンを使う。

現状の制限:

- Vault の切り替えや DB 設定の変更は Settings 側で行う。
- Refresh は全画面の主要データを読み直すが、細かい同期状態まではまだ表示しない。

## Navigation

左側の画面切り替え。

現在の画面:

- Home
- Inbox
- Projects
- Schedule
- Assistant
- Activity
- Settings

現状の制限:

- バッジ、件数、未読、警告などの状態表示はまだない。

実装メモ:

- Navigation / theme / delete / repair のアイコンには Google Material Icons を同梱して使う。
- Material Icons は Apache License 2.0。`crates/desktop/assets/fonts/MaterialIcons-Apache-2.0.txt` にライセンス文を置く。

## Inbox

未整理のタスクを素早く入れる場所。

使う場面:

- 思いついたことを忘れないように一旦入れる。
- プロジェクトや日程が未確定のタスクを仮置きする。
- `tomorrow`、`/due YYYY-MM-DD`、`45m`、`1.5h` などを含む短い入力からタスクを作る。

主な操作:

- Quick capture: 短い文章からタスク名、期限、見積時間を解釈して追加する。
- Task title / Due / Estimate: 明示的に入力してタスクを追加する。
- 一覧上の status / title / due / estimate / delete: 既存タスクのステータス変更、タスク名・期限・見積の行内編集、削除を行う。

この画面に置かないもの:

- 今日の時間割作成。
- プロジェクト全体の進捗管理。
- 保存済みスケジュールの編集。

現状の制限:

- AI による自動分類はまだ本格実装していない。
- Project / List / Milestone への直接割り当て UI はまだない。

## Home

今日のタスクと agenda を並べて扱う場所。

使う場面:

- 今日やる候補タスクを確認する。
- 左側でタスクを確認・編集する。
- 右側の agenda で1日の時間割を見る。
- Plan で agenda に予定案を作り、ScheduleBlock として保存する。
- 現在時刻ライン右端の repair アイコンから、現在時刻以降を再提案する。

主な操作:

- Tasks: 左パネルのタスク一覧。status button、所属、タスク名、期限、見積を表示する。
- Task density: Settings で Normal / Compact を選べる。Compact は Home のタスク行を詰めて表示する。
- Task sort: Home の Tasks 見出し右側で Due date / Estimate / Importance / Created を切り替える。Importance は現状 `cost_points` を使う。
- status button: 二重丸をクリックして status を選ぶ。メニューは status 名だけを表示し、Done 系 status を選ぶと完了扱いになる。
- status undo / redo: status 変更は `Ctrl+Z` で undo、`Ctrl+Shift+Z` で redo できる。
- title: タスク名をダブルクリックするとその場で入力欄に変わる。Enter またはフォーカスアウトで保存し、Esc でキャンセルする。
- due / estimate: クリックするとその場で背景なしの入力欄に変わる。候補は入力欄の下に floating popup として表示する。
- due / estimate popup: 候補テキストはhover時にポインター表示になり、クリックで即保存する。
- due: 通常入力時は近辺の日付セレクターをpopup表示し、`today`、`tomorrow`、`tue` など英語入力中は候補リストに切り替える。`YYYY-MM-DD`、`YYYYMMDD`、`YYYY/MM/DD` も解釈する。
- estimate: `30m`、`1h30m`、`01:30`、`01:30:00`、分数を解釈する。固定値と加算ボタンをpopup内から選べる。
- Done toggle: 左パネル下部の三角アイコンで完了済みタスクを表示できる。直近10件から表示し、必要に応じて追加読み込みする。完了済みタスクにも status button を出し、戻し操作ができる。
- delete icon: Material Icons を使う。ホバー時に明るくなり、確認後に削除する。
- Task drag: タスク行を agenda にドラッグすると、ドロップ位置の時刻に manual / locked な ScheduleBlock として保存する。開始時刻は15分単位に丸める。
- Block drag: agenda 上の保存済み timeblock をドラッグすると開始時刻を変更できる。移動後は Plan / Repair / Manual 由来に関わらず manual / locked な ScheduleBlock として保存する。
- Agenda date: 右パネル上部で対象日を切り替える。
- Today: すでに当日が表示されている状態でクリックすると、agenda の現在時刻ラインへスクロールする。
- Home open: Home 表示時は対象日が今日なら現在時刻ライン付近へ自動スクロールする。
- Plan: 対象日のタスクから agenda に予定案を作り、proposed ScheduleBlock として保存する。既存 proposed block がある場合は置き換え確認を出す。
- Repair: Plan の横にあるボタン。現在時刻以降を再提案し、結果を自動保存する。
- Repair icon: 現在時刻ライン右端のアイコン。hover で強調され、クリックすると現在時刻以降を repair する。
- Schedule result: proposed / unscheduled / issue 件数に加え、issue の内容を短いメッセージとして表示する。
- Block color: Plan 由来、Repair 由来、Manual 由来の block は agenda 上で色を分ける。

この画面に置かないもの:

- 月全体の予定確認。
- プロジェクト横断の長期ロードマップ。
- AutomationLog の監査。

現状の制限:

- availability は現時点では既定の 9:00-17:00 を使う。ただし過去日には計画せず、当日は現在時刻以降だけを対象にする。
- Repair は現在時刻以降の proposed block を置き換える。差分レビュー UI はまだない。
- 外部カレンダー予定はまだ取り込んでいない。

## Schedule

保存済み ScheduleBlock を確認・調整する場所。

使う場面:

- Home で保存した予定案を時間割として見る。
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
- Timezone offset を設定する。既定は JST の `+09:00`。`JST` / `Asia/Tokyo` / `UTC` も入力できる。
- Home task density と既定 sort を設定する。
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
- Home と Schedule の役割が混ざって見えないか。
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
