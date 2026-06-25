# Mnema Desktop 起動・配布メモ

作成日: 2026-06-26

## 現在の位置づけ

現在の desktop app は、egui / eframe によるローカルファーストの MVP である。

主に触れる範囲:

- Inbox: quick capture とタスク追加。
- Today: 今日の候補タスク、計画生成、ScheduleBlock 保存。
- Today repair: scheduled / done / active / locked block を固定し、proposed block を再提案。
- Projects: Project / List / Milestone の追加と選択。
- Schedule: 日別タイムライン、ScheduleBlock の状態・時刻編集。
- Assistant: LLM provider 設定時の簡易相談、またはタスク作成。
- Activity: AutomationLog の確認。
- Settings: Vault、SQLite、PostgreSQL、LLM、テーマ設定。

## 開発起動

PowerShell から以下を実行する。

```powershell
.\scripts\dev-desktop.ps1
```

既定では `.\vault` を Vault として使い、SQLite DB はアプリ既定の local app data 配下に置く。
Vault を変える場合:

```powershell
.\scripts\dev-desktop.ps1 -VaultPath C:\path\to\vault
```

PostgreSQL を使う場合:

```powershell
.\scripts\dev-desktop.ps1 -Backend postgres -DatabaseUrl postgres://postgres:postgres@localhost/mnema
```

## Windows 簡易パッケージ

release build と最小ファイルを `dist\mnema-desktop-windows` にまとめる。

```powershell
.\scripts\package-desktop-windows.ps1
```

作成後は以下で起動できる。

```powershell
.\dist\mnema-desktop-windows\run-mnema.ps1
```

この簡易パッケージはインストーラーではない。署名、auto-update、ショートカット作成、Windows 通知連携はまだ含めない。

## DB 方針

既定は SQLite。Docker / PostgreSQL なしで触れることを優先する。

SQLite DB は Vault 外に置くことで、Dropbox Desktop / Google Drive Desktop などによる同期フォルダ内 DB ファイルロックを避ける。

PostgreSQL は self-hosted / server 運用向けの任意 backend として残す。

## まだ残る作業

- Calendar / Gantt の本格ビュー。
- Assistant からの計画修正と承認 UI。
- AutomationLog からの undo / review UI。
- Windows インストーラー、署名、auto-update。
- モバイル / Web クライアントの再評価。
