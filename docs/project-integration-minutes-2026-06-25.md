# Mnema / resched 統合検討メモ

作成日: 2026-06-25

## 背景

`mnema` と、新規案である `resched-repo-skeleton/resched` はどちらも個人向けのタスク・計画支援を扱う。
一方で、重心が異なるため、そのまま並行開発するとドメインモデル、AI の責任範囲、スケジューリング方針が競合しやすい。

このメモは、両プロジェクトの目的・特性を整理し、統合後の開発方針を後から参照できる形で残すための議事録である。

## Mnema の特性

Mnema は「頭の中にタスクを残さない」ための、ローカルファーストな AI 付きタスク記憶アプリである。

主な目的:

- 思いついたタスクやメモを frictionless に Inbox へ投げ込めること。
- タスクを Project / List / Milestone / Status に整理すること。
- AI 秘書が分類、期日提案、スケジュール提案、週次レビューを支援すること。
- ユーザーの好みや作業スタイルを LLM memory として蓄積すること。
- 自動処理は AutomationLog に残し、レビューやロールバックの余地を持たせること。

既存実装の状態:

- Rust workspace は `crates/core`, `crates/infra`, `crates/desktop` 構成。
- `core` に Task, Project, List, Milestone, Status, Assistant, AutomationLog, UserSettings などのモデルがある。
- `infra` にDB repository、LLM abstraction、automation queue の初期実装がある。
- Desktop は現時点では Vault 起動確認用の stub。

Mnema の強み:

- アプリ全体の世界観と情報設計がある。
- AI 秘書、Vault、AutomationLog まで含めた「タスク記憶」の設計がある。
- 既に永続化層とテストがあるため、アプリの母体にしやすい。

## resched の特性

resched は「予定が崩れた後に、現実的な計画へ戻る」ための adaptive planner / schedule repair engine である。

主な目的:

- Task, Habit, Calendar, Estimate, RunLog を統合して Today plan を作ること。
- 外部予定や availability window を制約として扱うこと。
- p50 / p80 / p95 の見積もり分布を使い、過密計画を見える化すること。
- 予定が遅れた、割り込みがあった、低エネルギー状態になった場合に repair suggestions を出すこと。
- LLM は決定主体ではなく、自然言語入力・分解・説明・レビューの補助として使うこと。

既存実装の状態:

- Rust workspace は `domain`, `scheduler`, `storage`, `calendar`, `estimation`, `agent`, `cli` に分かれている。
- `scheduler` に greedy scheduler の初期実装とテストがある。
- `domain` に TimeWindow, Estimate, ScheduleBlock, AvailabilityWindow, ExternalEvent などがある。
- storage や calendar はまだ境界定義に近い。

resched の強み:

- scheduler-first / deterministic という安全な判断軸が明確。
- Task と ScheduleBlock を分離しており、予定変更や repair diff を扱いやすい。
- 見積もり誤差、実績ログ、外部カレンダー、低エネルギー状態など、実行段階の問題に強い。

## 主な競合点

### AI の責任範囲

Mnema は AI 秘書が能動的に支援する世界観が強い。
resched は `LLM proposes. Scheduler validates. User approves.` を原則にしている。

統合後は resched 側の方針を採用する。
AI は提案、説明、自然言語処理、レビュー文面を担当し、予定変更の決定主体にはしない。

### priority の扱い

Mnema は priority を永続化せず、期日・見積もり・依存関係などから導出する方針である。
resched は scheduler 入力として Priority を持っている。

統合後は Mnema の方針を維持する。
永続フィールドとしての priority は避け、scheduler 内部の ranking score として導出する。
ただし、hard deadline、importance hint、energy required など、順位計算に必要な入力は追加できる。

### status model

Mnema は `NotStarted / InProgress / Pending / Done` の大枠を持つ。
resched は `Inbox / Ready / Scheduled / Active / Waiting / Blocked / Paused / Done / Dropped` の scheduler 向けカテゴリを持つ。

統合後は二層に分ける。

- 表示・カスタム管理用: Mnema の Status / StatusGroup。
- scheduler 判断用: resched 由来の normalized status category。

### Project / Gantt と Today / Repair の優先順位

Mnema は Project, Milestone, Gantt を重視している。
resched は Today view と Repair mode を MVP の中心に置く。

統合後の MVP は Today / Repair を優先する。
Gantt は Project 分析・長期計画ビューとして後続に回す。

## 統合方針

統合案は、Mnema をアプリ本体として残し、resched をスケジューリング・修復エンジンとして取り込む形にする。

統合後のプロダクト定義:

> Mnema は、タスクを記憶し、今日の実行計画を作り、崩れた予定を短時間で修復するローカルファーストな個人用 planning assistant。

採用する設計原則:

- Local-first を維持する。
- Scheduler は deterministic にする。
- LLM output は untrusted suggestion として扱う。
- 予定変更は diff として表示し、ユーザー承認を前提にする。
- Task と ScheduleBlock は分離する。
- 外部カレンダーは初期は read-only ingestion にする。
- Habit は recurring task ではなく独立 entity として扱う。

## 開発方針

短期的には、Mnema 側に resched の概念を段階的に取り込む。

優先順:

1. SQLite を既定、PostgreSQL を任意 backend とする永続化層へ切り替える。
2. `crates/scheduler` を追加し、greedy scheduler を Mnema の domain と接続する。
3. `ScheduleBlock`, `AvailabilityWindow`, `ExternalEvent`, `RunLog`, `AgentSuggestion` を core / infra に追加する。
4. `PlanTodayService` と `RepairScheduleService` を作る。
5. Desktop UI は Gantt より先に Today view と Repair mode を作る。
6. LLM 秘書は capture, clarify, decomposition, explanation, review を担当する。

## DB backend 方針について

当初の Mnema 実装は SQLite を Vault 内に置く設計だった。
その後、sync / server / analytics / multi-surface 展開を見据えて PostgreSQL を正本DBに寄せる案を検討した。

ただし、初期ユーザーがアプリ単体で触る段階では、PostgreSQL 前提にすると Docker または PostgreSQL インストールが必要になり、導入摩擦が大きい。
そのため、現時点の折衷案としては以下を採用する。

- 既定 backend は SQLite。
- SQLite DB は Vault / Dropbox / Google Drive などの同期フォルダ内に置かず、OS の local app data 配下に置く。
- PostgreSQL は `MNEMA_STORAGE_BACKEND=postgres` と `MNEMA_DATABASE_URL` で任意選択できる backend として残す。
- Vault は attachment, export, assistant assets, local config などのファイル群の root として残す。

この方針により、ローカルアプリとしてはDBインストールなしで開始でき、self-hosted / server 展開が必要なユーザーは PostgreSQL を選べる。

PostgreSQL を残す理由:

- 後続の sync / server / analytics / multi-surface 展開に寄せやすい。
- schedule blocks, logs, suggestions などの履歴系テーブルが増えた時に扱いやすい。
- JSONB や timestamp 型を使いやすい。

SQLite を既定にする理由:

- アプリだけで完結しやすく、初回導入に Docker / PostgreSQL が不要。
- 単一ユーザーのローカル利用では十分に扱いやすい。
- DB ファイルを同期フォルダ外に置けば、Dropbox Desktop / Google Drive Desktop などによるファイルロック懸念を避けやすい。

## 当面の非目標

- チーム向け権限管理。
- 完全な ClickUp clone。
- カレンダー write-back の既定有効化。
- LLM による予定の自動変更。
- 医療的診断や ADHD / ASD の推論。
- 高度な solver の導入。

## 決定事項

- アプリ名・母体は `mnema` を継続する。
- `resched` は独立アプリではなく、scheduler / repair の設計資産として吸収する。
- AI-first ではなく scheduler-first に寄せる。
- MVP は Inbox, Today view, Schedule repair, Actual logging, Estimate learning を優先する。
- Gantt, Habit, Calendar write-back, Sync は後続フェーズに回す。
