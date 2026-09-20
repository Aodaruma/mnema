# Mnema Server

Mnema の既存 `core` / `app` / `infra` を利用する、単一バイナリの REST API + Web GUI です。静的ファイルはバイナリへ埋め込まれるため、実行時に Node.js や別の Web server は不要です。

## 起動

```powershell
cargo run -p mnema-server
```

既定では `http://127.0.0.1:8080` で Web GUI と API を配信します。

## REST API

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/health` | health、version、backend、Web の既定値 |
| `GET` | `/api/tasks` | soft-delete されていないタスク一覧 |
| `POST` | `/api/tasks` | Inbox タスク作成 |
| `GET` | `/api/tasks/{id}` | タスク取得 |
| `PUT` | `/api/tasks/{id}` | タイトル・メモ・期限・見積・完了状態の更新 |
| `DELETE` | `/api/tasks/{id}` | タスクの soft delete |
| `POST` | `/api/plans/today/preview` | Today plan のプレビュー |
| `POST` | `/api/plans/today/apply` | Today plan を proposed ScheduleBlock として保存 |
| `GET` | `/api/schedule?date=YYYY-MM-DD` | 指定日の ScheduleBlock 一覧 |
| `GET/PUT` | `/api/scheduling/preferences` | IANA timezone、Hours、Sleep、Travel buffer |
| `GET/POST` | `/api/habits` | Habit 一覧・追加 |
| `PUT` | `/api/habits/{id}` | Habit 編集 |
| `POST` | `/api/habits/{id}/disable` | Habit 無効化 |
| `POST` | `/api/habits/occurrences/expand` | 日付範囲の occurrence を冪等展開 |
| `POST` | `/api/habits/occurrences/{id}/skip` | occurrence を skip |
| `POST` | `/api/habits/occurrences/{id}/snooze` | occurrence を RFC 3339 時刻まで snooze |
| `POST` | `/api/auto-schedule/preview` | 既定7日間の Task/Habit 汎用差分 preview |
| `POST` | `/api/auto-schedule/apply` | preview fingerprint を再検証して適用 |
| `POST/GET` | `/api/calendar/oauth/start`, `/api/calendar/oauth/callback` | Google OAuth PKCE |
| `GET` | `/api/calendar/accounts` | 接続 account 一覧 |
| `GET/PUT` | `/api/calendar/accounts/{id}/calendars`, `.../selection` | provider Calendar 一覧・取り込み選択 |
| `POST` | `/api/calendar/accounts/{id}/sync` | full/incremental event sync。body の `force_full: true` で full sync を強制 |
| `POST` | `/api/calendar/accounts/{id}/managed-calendar` | Mnema 専用 Calendar を確保 |
| `POST` | `/api/calendar/accounts/{id}/writeback` | ScheduleBlock を create/update/delete |

Plan request の例:

```json
{
  "date": "2026-08-15",
  "availability_start": "09:00",
  "availability_end": "17:00",
  "timezone_offset": "+09:00"
}
```

Plan は保存済み block のうち `locked`、`scheduled`、`active`、`done`、外部カレンダー由来を busy time として扱います。`apply` が置換するのは対象日の proposed block だけです。

## 環境変数

| Name | Default | Description |
| --- | --- | --- |
| `MNEMA_SERVER_ADDR` | `127.0.0.1:8080` | listen address。container では `0.0.0.0:8080` を指定 |
| `MNEMA_VAULT_PATH` | `./vault` | Vault root |
| `MNEMA_STORAGE_BACKEND` | `sqlite` | `sqlite` または `postgres` |
| `MNEMA_SQLITE_PATH` | OS の local app data | SQLite DB path |
| `MNEMA_DATABASE_URL` | `postgres://postgres:postgres@localhost/mnema` | PostgreSQL 接続 URL |
| `MNEMA_TIMEZONE` | `Asia/Tokyo` | IANA timezone。DST を含む日付境界・Hours・Sleep に利用 |
| `MNEMA_TIMEZONE_OFFSET` | `+09:00` | 旧 Today plan API 用。`UTC` / `JST` / offset を後方互換で維持 |
| `MNEMA_PLANNING_START` | `09:00` | Web の既定 planning window 開始 |
| `MNEMA_PLANNING_END` | `17:00` | Web の既定 planning window 終了 |
| `MNEMA_WEB_REFRESH_SECONDS` | `30` | Web GUI の既定自動更新秒数 |
| `MNEMA_AUTOMATION_MODE` | `off` | `off` / `suggest` / `auto_silent`。background apply は明示的な `auto_silent` のみ |
| `MNEMA_AUTOMATION_INTERVAL_SECONDS` | `0` | `0` は worker 無効。正数時は起動時と interval ごとに calendar sync と7日プランを実行 |
| `MNEMA_AUTOMATION_ASSISTANT_ID` | なし | 既存 Assistant の UUID 指定時のみ worker の成功・警告・失敗を `AutomationLog` へ best-effort 記録 |
| `MNEMA_GOOGLE_CLIENT_ID` | なし | Google OAuth client ID。未設定時 Calendar provider API は 503 |
| `MNEMA_GOOGLE_CLIENT_SECRET` | なし | confidential client の場合のみ |
| `MNEMA_GOOGLE_REDIRECT_URI` | なし | 例: `http://127.0.0.1:8080/api/calendar/oauth/callback` |
| `MNEMA_CREDENTIAL_KEY` | なし | headless 用。32-byte AES key の Base64 文字列。指定時は token を Vault 内 `.credentials` へ暗号化保存 |
| `MNEMA_CREDENTIAL_KEY_FILE` | なし | 上記 Base64 key を格納した file path。`MNEMA_CREDENTIAL_KEY` が優先 |
| `RUST_LOG` | `mnema_server=info` | tracing filter |

初回起動時は `local_user_id()` に対して default `SchedulingPreferences` を作成します。Web の Hours / Sleep / Travel / timezone は DB へ、theme と自動更新間隔は browser の `localStorage` へ保存します。

OAuth token は DB やログへ保存しません。既定では OS の secure store (`KeyringCredentialStore`) を利用します。headless 環境では `MNEMA_CREDENTIAL_KEY` または `MNEMA_CREDENTIAL_KEY_FILE` を設定すると、Vault 内 `.credentials` に AES 暗号化して保存します。key は token と同じ volume に置かず、secret manager などから注入してください。

worker は有効 account の選択済み provider calendar を full/incremental sync した後、7日間を preview します。rolling horizon が固定されないよう、最後の full sync から12時間以上経過した calendar は再び full sync します。sync に失敗した account は警告を残し、最後に永続化できた `ExternalEvent` snapshot で preview を続行します。`suggest` はここで終了し、ScheduleBlock の apply と calendar write-back は行いません。`auto_silent` を明示した場合だけ apply し、続いて `READ_WRITE` かつ managed calendar 設定済みの有効 account へ冪等 write-back します。

## Web 構成

- `web/js/api.js`: 現行 REST client
- `web/js/store.js`: browser state / preferences
- `web/js/ui.js`: view renderer
- `web/js/app.js`: event と refresh orchestration
- `web/js/features/calendar.js`: OAuth / account / selection / sync / writeback client
- `web/js/features/habits.js`: Habit / occurrence client

Auto Schedule の `preview` は HabitOccurrence を含め保存を一切変更しません。新しい occurrence と ScheduleBlock は `apply` 時にまとめて保存します。`apply` には直前 preview の `fingerprint` が必須で、Calendar・Hours 等が変化していれば `409 Conflict` となります。

## セキュリティ上の注意

現時点では認証を提供しません。ローカル利用では既定の loopback bind を維持してください。LAN やインターネットへ公開する場合は、認証・TLS・アクセス制御を行う reverse proxy の背後に配置してください。
