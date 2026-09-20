# Mnema 現行UI・操作仕様

確認日: 2026-09-21

Scheduling MVP alpha のデスクトップ画面と、コード上の操作範囲を整理したメモです。
短期の開発方針は [development-plan.md](development-plan.md) を参照してください。
6月版のUIガイドにある「Plan / Repairの自動保存」などは、現在の画面と異なります。

## 確認した状態

- 現行ソースから `cargo build -p mnema-desktop --locked` でビルドし、GUIを起動。
- SQLiteへの接続、Home / Schedule / Habits / Calendar / Settingsの表示を確認。
- Scheduleの `Preview plan` で提案を生成し、HomeのAgendaに未保存ブロックとして表示されることを確認。
- `Apply`、タスク編集、Habit追加、設定保存、Google認証は今回実行していません。
- ワークスペースの自動テスト111件と `cargo fmt --all -- --check` は成功。

## 画面と役割

| 画面 | 現在の役割・操作 |
| --- | --- |
| Home | 左にタスク、右に日別Agenda。並べ替え、行内編集、完了済みタスクの展開、Plan、未保存提案のApply、Repair preview。手動配置・保存済みブロックのドラッグ移動も実装されている。 |
| Inbox | Quick captureとタイトル・期限・見積時間によるタスク追加。`tomorrow`、`/due`、`45m`等の簡易入力を解釈する。既存タスクの行内編集も可能。 |
| Projects | Project / List / Milestoneの作成・表示、Details / Gantt切替。Ganttの直接編集や依存関係表示は後続。 |
| Schedule | 保存済み予定をDay / Calendar（月間）で表示。日付切替、Preview plan、Apply、ブロックの状態・時刻編集。 |
| Calendar | Google接続状況、取り込み対象calendar ID、同期状況を表示。未接続時はWebサーバーへの案内とURLコピーボタンを表示する。 |
| Habits | 毎日／曜日指定、所要時間、Required / Flexible、希望時間帯で習慣を追加。発生分のSkip / Snooze 24h、習慣のDisableを扱う。 |
| Assistant | 設定済みLLMへの簡易相談とタスク作成補助。会話による計画変更の承認フローは未完成。 |
| Activity | AutomationLogを表示。履歴全体のUndo / Rollback / Review操作は未完成。 |
| Settings | Vault、SQLite / PostgreSQL、接続先、タイムゾーン、作業時間、睡眠、移動バッファ、Homeの表示密度・並べ替え、LLM接続先とモデルを設定。 |

上部には接続backend、Refresh、Light / Dark切替があり、左側のナビゲーションで画面を切り替えます。
UIのラベルは現在、英語と日本語が混在しています。

## Planの通常操作

1. Inboxでタスクと見積時間を入力する。
2. Settingsで作業時間・睡眠・タイムゾーンを設定し、必要ならHabitsを追加する。
3. Homeの `Plan` またはScheduleの `Preview plan` で提案する。GUIの対象期間は1 / 3 / 7日。
4. HomeのAgendaやScheduleの未保存プレビュー概要で、提案件数・変更件数・未配置件数を確認する。
5. Homeの `Apply preview` またはScheduleの `Apply` でローカルDBへ反映する。

通常のPlanはTask / Habit、保存済みの保護ブロック、同期済み外部予定、睡眠、作業時間、移動バッファを扱います。
Preview自体では予定やHabitOccurrenceを保存しません。Apply時にfingerprintで提案の前提を再確認します。
ローカルApplyとGoogle Calendarへの書き戻しは別の操作です。

手動ドラッグはプレビュー経由ではなく、その場でmanual / lockedのブロックとして保存する実装です。
予定の状態は `proposed / scheduled / active / done / missed / cancelled` を持ちます。

## 操作面ごとの違い

- デスクトップのCalendar画面内ではGoogle OAuthを完結できません。現在の案内先はWebサーバーの `http://127.0.0.1:8080/#settings` です。
- デスクトップだけを起動してもWebサーバーは起動しません。Web操作には別途 `cargo run -p mnema-server --locked` が必要です。
- Google接続、同期、専用カレンダーの作成・書き戻しはWeb / CLI / REST側に実装されています。Google実アカウントでの疎通は未確認です。
- ドメインは曜日別・複数区間の時間設定を持ちますが、デスクトップSettingsは作業開始／終了と睡眠開始／終了を中心とした簡易フォームです。
- Homeの `Repair preview` は現在、旧 `RepairScheduleService::repair_day` を呼び出します。Task / Habit共通の `AutoScheduleService` による通常Planとは別経路で、適用導線や制約の統一は今後の確認項目です。
- Webサーバーは現時点でアプリ内認証を持ちません。公開運用の構成は [server README](../crates/server/README.md) を参照してください。

## 今後のUI調整で確認したい点

- 初期ウィンドウ幅（1120px設定）ではHome上部の操作列が収まらず、一部のボタンが右側に隠れる。最大化時は表示できる。
- Settingsも初期ウィンドウの高さでは全項目と保存ボタンが見渡せない。レイアウトとスクロール導線を確認する。
- Home / Scheduleの役割、Plan / Repair / Applyの違い、未保存状態の伝え方を整理する。
- Calendar接続をWebへ案内する際、サーバー起動・必要設定まで分かる導線を用意する。
- 変更件数・fingerprint中心の提案概要を、移動前後の時刻や理由が分かる表示へ改善する。
- 習慣・外部予定・睡眠・移動時間をAgenda上でどう区別するかを整理する。

このメモは現状確認であり、上記のUI・機能変更は実施していません。

## 起動方法

リポジトリ直下から:

```powershell
.\scripts\dev-desktop.ps1
```

または:

```powershell
cargo run -p mnema-desktop --locked
```

今回起動した実行ファイルは `target/debug/mnema-desktop.exe` です。
`dist/mnema-desktop-windows` の6月版配布物は更新していません。
