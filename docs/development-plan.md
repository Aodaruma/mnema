# Mnema Development Plan v0.2 — Auto Scheduling First

更新日: 2026-08-15
状態: **現在の短期開発計画（source of truth）**

## 1. 決定

Mnema 全体を一度に完成させる開発は止め、まず自動スケジューリングを独立した縦のスライスとして完成させる。

短期プロダクトを次のように定義する。

> Mnema Scheduling MVP は、外部予定・生活制約・習慣を守りながら、タスクを自動配置・再配置するローカルファーストな個人用スケジューラ。

別リポジトリへ fork せず、既存 workspace の `core / scheduler / app / infra / desktop` 境界を利用する。後で Project、Assistant、AutomationLog などから同じ application service を呼び出せる形に保つ。

操作面は headless な CLI を最小基準とし、同じ application service を desktop GUI、server REST API、Web GUI から利用する。server は単一バイナリとコンテナの双方で self-host できる形にする。

## 2. 現在地

現在は「初期設計」ではなく、**Scheduling MVP alpha / S0〜S3 実装・統合検証済み**に相当する。

実装済み:

- SQLite 既定、PostgreSQL 任意の永続化
- Task と ScheduleBlock のドメインモデル
- DB・UI・LLM に依存しない決定的 greedy scheduler
- Today Plan、ScheduleBlock 保存、Repair
- 手動ブロック移動、日表示、月表示
- Inbox、Project、Assistant、Activity 等の既存画面
- Task / Habit 共通scheduler、IANA timezone / DST、overlap取得、preview / fingerprint / apply
- Google OAuth PKCE、複数calendar選択、full / incremental sync、all-day / cancelled / transparent event
- 曜日別Hours、睡眠hard protection、Habit展開・skip / snooze / disable、travel override
- 専用calendarへのETag付き冪等write-back、7日rolling worker、`AutoSilent` mutation gate
- headless CLI、desktop GUI、REST API、responsive Web GUI、単一server binary、Docker Compose
- OS Keyringと、headless向けAES-256-GCM暗号化credential file

残る実環境確認:

- Google実アカウントでのOAuth・Calendar API疎通
- OS Keyringの実保存と、container secretを用いた暗号化credential file
- PostgreSQL実DB接続でのS0〜S3シナリオ
- Docker imageのlive build / 起動（Compose構文は検証済み）
- 認証・TLS付きreverse proxyを含む外部公開構成

## 3. 開発原則

1. Scheduler は deterministic に保ち、LLM を必須経路に入れない。
2. 外部予定、睡眠、locked block は hard constraint として絶対に重ねない。
3. Habit と Task は移動可能な item とし、優先度と許容時間帯を明示する。
4. 自動変更は冪等にし、同じ入力から重複 event を作らない。
5. 初期既定は preview + user approval とし、silent apply は明示設定時だけ許可する。
6. 外部カレンダーの既存 event は編集しない。write-back は Mnema 専用 calendar に限定する。
7. OAuth token は DB や平文設定へ保存せず、OS の資格情報ストアを利用する。
8. 保存時刻は UTC、利用者の規則は IANA timezone とローカル時刻で表現する。
9. 大規模 refactor は行わず、各 stage を単独で利用・検証できる状態にする。

## 4. MVP の範囲

含める:

- Google Calendar 1 provider を初期対象とする read-only 同期
- 選択した複数 calendar の busy event 取込
- 曜日別の Work / Personal / Sleep 等の時間ポリシー
- 毎日または曜日指定の簡易 Habit、所要時間、許容時間帯、必須度
- location または明示フラグ付き予定の固定 before / after travel buffer
- Task と Habit の自動配置、外部予定変更後の Repair
- 提案差分の確認、Apply / Cancel
- Mnema 専用 calendar への承認付き write-back
- 起動時、手動同期後、設定時刻の再計画
- headless CLI による全主要操作
- REST API と、desktop と同じ主要導線を持つ responsive Web GUI
- 単一server binaryとDockerによるself-host deployment

初期非目標:

- Outlook / CalDAV 等を含む複数 provider 対応
- Maps API による経路・所要時間推定
- Reclaim.ai 全機能の再現
- 会議参加者の最適化、Scheduling Link、チーム機能
- 完全な RFC 5545 recurrence editor
- 高度な数理 solver、energy 推定、健康状態の推論
- 外部 calendar の既存 event の自動移動
- AI 秘書、週次レビュー、Gantt、モバイル、同期機構の追加開発

最後の項目群は削除せず、Scheduling MVP 完成まで凍結する。

## 5. 境界とデータフロー

```text
Calendar adapter ──> ExternalEvent ──────────────┐
Saved/locked blocks ─────────────────────────────┤ hard busy
Sleep/life rules ──> Availability / protection ─┤
Habit definition ──> HabitOccurrence ────────────┤ schedulable
Task ────────────────────────────────────────────┤ schedulable
Travel policy ──> Buffer blocks ─────────────────┤
                                                  v
                                      deterministic scheduler
                                                  v
                                      proposed plan + diff
                                                  v
                                local apply / calendar write-back
```

責務:

- `core`
  - `Habit`、`HabitOccurrence`、`ExternalEvent`、`SchedulingPreferences` 等の provider 非依存モデル
  - repository port
- `scheduler`
  - `SchedulableItem`、availability、hard busy、soft preference から提案を生成する純粋ロジック
  - DB、OAuth、HTTP、UI、recurrence 展開を持たない
- `app`
  - calendar event、既存 block、生活規則、Habit occurrence を scheduler input へ組み立てる
  - `CalendarSyncService`、`GenerateHabitOccurrencesService`、`AutoScheduleService`
  - proposal diff と apply の orchestration
- `infra`
  - SQLite repository、Google Calendar adapter、同期 cursor、credential store
  - 外部 event と Mnema managed event の対応表
- `desktop`
  - 接続、同期、Habit / Hours 編集、提案レビューの薄い UI
  - Scheduling UI を `gui.rs` から段階的に module 分割し、後で本体へそのまま統合可能にする
- `cli`
  - GUIなしで Task、Calendar、Habit、Plan、Apply、Repair、server設定を操作する基準interface
- `server`
  - `app` serviceを公開するREST API、定期trigger、静的Web GUI配信
  - desktop固有状態やUIロジックを持たない
- `web`
  - server APIだけを利用し、Home / Inbox / Schedule / Calendar / Habits / Settingsを提供

## 6. 開発 stages

### S0: 現行スケジューラの安全化

状態: **実装済み**

実装:

- `list_for_day(Date)` 中心の API を `list_overlapping(TimeWindow)` へ寄せる。
- app 層で IANA timezone のローカル日を UTC range へ変換する。
- 通常 Plan でも manual / locked / scheduled / active / done / external block を busy にする。
- scheduler の item 識別子を Task 専用から `Task | HabitOccurrence` に一般化する。
- Plan / Repair を preview と apply に分離し、既定の自動保存を止める。
- desktop の Scheduling 関連コードを小さな module へ抽出し始める。

完了条件:

- JST の日付境界と跨日 block を正しく取得できる。
- 既存の確定 block と提案が重ならない。
- Preview を Cancel した場合、DB が変化しない。
- 同じ入力から同じ順序・時刻の提案が得られる。
- 既存テスト35件を維持し、境界・重複・跨日の回帰テストを追加する。

### S1: Calendar read-only vertical slice

状態: **実装済み（Google実アカウント疎通は未確認）**

初期 provider は Google Calendar とする。実利用先が異なる場合は、この stage の adapter だけを置き換える。

実装:

- 最小権限の OAuth と calendar 選択 UI
- provider 非依存の `CalendarGateway`
- `calendar_accounts`、`external_events`、`sync_cursors` 相当の永続化
- 初回 full sync と sync token を用いた incremental sync
- cancelled event、recurrence instance、all-day、timezone、free / busy の取扱い
- 外部 event の冪等 upsert と `BusyBlock` 変換
- 手動 Sync とアプリ起動時 Sync

完了条件:

- 選択 calendar の busy event が Plan / Repair に反映される。
- 同期を繰り返しても重複しない。
- 変更・削除された event が次回同期で反映される。
- transparent event は既定で空き時間を塞がない。
- ネットワーク断時は最後に同期済みの snapshot で計画できる。

### S2: Habit、睡眠、生活時間

状態: **実装済み**

実装:

- 曜日ごとに複数区間を持てる named scheduling hours
- Sleep を既定 hard protection とする recurring rule
- 食事・運動・家事等を fixed routine または flexible Habit として表現
- Habit の daily / selected weekdays、duration、earliest / latest、required / flexible
- 対象 horizon への HabitOccurrence の冪等な展開
- skip / snooze / disable
- hard constraints → required habits → flexible habits → tasks の決定的な割当順

完了条件:

- 睡眠時間へ Task / Habit を配置しない。
- 曜日別・複数区間の availability が反映される。
- 同じ Habit を再展開しても occurrence が重複しない。
- Habit の skip / snooze が再計画後も維持される。
- Task が入らない場合と Habit が入らない場合を区別して説明できる。

### S3: Travel buffer、自動化、安全な write-back

状態: **実装済み（Google実write-backとDocker live buildは未確認）**

実装:

- location または `needs_travel` 指定の event 前後へ固定 buffer を生成
- 全体既定値と event ごとの override
- 7日 rolling horizon の再計画
- 起動時、calendar sync 後、指定時刻の trigger
- proposal diff に create / move / resize / remove と理由を表示
- Mnema 専用 calendar の作成と managed event の冪等 upsert
- calendar event ID / etag と ScheduleBlock の対応表
- apply 結果と失敗を AutomationLog へ記録
- CLIからsync / preview / apply / repair / Habit / Hoursを操作
- server REST API、定期worker、responsive Web GUI
- 単一binaryとDocker deployment

完了条件:

- 移動対象 event の前後に設定した buffer が入り、他の item と重ならない。
- 外部予定の時刻変更後、保護済み block を壊さず再提案できる。
- Apply を複数回実行しても managed event が重複しない。
- write-back 失敗時にローカル状態と同期状態を区別して再試行できる。
- silent apply は `AutoSilent` を明示した場合だけ動く。

### S4: 安定化と Mnema 全体への再統合

実装候補:

- Planner UI を既存 Home / Schedule へ統合
- undo / review 履歴の完成
- PostgreSQL の scheduling feature parity
- タスク分割、依存関係、見積学習、通知
- LLM による入力補助・説明。ただし scheduler の決定は変更させない。

完了条件:

- Scheduling MVP の application service を Assistant や Project から再利用できる。
- provider adapter を追加しても scheduler の変更が不要である。
- frozen features の再開が Scheduling MVP のデータモデルを複製しない。

## 7. リリース判定

最初の実用版は S0〜S3 完了時点とする。進捗は日数ではなく、以下の必須シナリオと各stageの完了条件で判定する。OAuth provider の審査・公開手続き、Maps API、複数 provider 対応は含めない。

実用版の必須シナリオ:

1. Google Calendar を接続して対象 calendar を選ぶ。
2. 曜日別の作業時間、睡眠、Habit、固定 travel buffer を設定する。
3. 外部予定と重ならない7日分の提案を生成する。
4. 差分を確認してローカルまたは Mnema 専用 calendar へ反映する。
5. 外部予定を変更し、重複なしで Repair する。
6. アプリ再起動・再同期・再 Apply 後も event が重複しない。

## 8. 旧計画との関係

- `docs/specification-v0.1.md` は長期的なプロダクト構想として残す。
- `docs/project-integration-minutes-2026-06-25.md` は意思決定の履歴として残す。
- `docs/docs/development-plan.md` と `docs/remaining-implementation-2026-06-25.md` は当時の snapshot とする。
- 短期の優先順位と完了条件が衝突する場合は、この文書を優先する。

## 9. 参考にした外部仕様

- Reclaim.ai の Hours は Work / Meeting / Personal / Custom の時間ポリシーを持つ: <https://help.reclaim.ai/en/articles/3600766-set-your-working-meeting-personal-custom-hours>
- Reclaim.ai の buffer は固定 travel time、decompression、Task / Habit breaks を扱う: <https://help.reclaim.ai/en/articles/4281992-buffer-time-overview-travel-decompression-and-tasks-habit-breaks>
- Google Calendar は最小権限の OAuth scope を選択できる: <https://developers.google.com/workspace/calendar/api/auth>
- Google Calendar の incremental sync は sync token を永続化し、無効化時は full sync をやり直す: <https://developers.google.com/workspace/calendar/api/guides/sync>
