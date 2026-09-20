# Mnema container example

リポジトリ root を build context にして起動します。

```powershell
docker compose -f .\deploy\compose.yaml up --build
```

起動後は `http://127.0.0.1:8080` を開きます。SQLite DB と Vault は named volume `mnema-data` に保存されます。

この compose はローカル利用向けで、host port を loopback に限定しています。外部公開する場合は認証と TLS を備えた reverse proxy を追加してください。全環境変数は `crates/server/README.md` を参照してください。

Google Calendar を使う場合は `MNEMA_GOOGLE_CLIENT_ID` と `MNEMA_GOOGLE_REDIRECT_URI`（callback は `/api/calendar/oauth/callback`）を追加します。OAuth token は既定で OS keyring に保存されるため、headless container では Secret Service 等の keyring backend が必要です。未設定時、Calendar provider 操作は明確な `503 Service Unavailable` となり、Task/Habit/Auto Schedule は引き続き利用できます。

自動 worker は既定で無効です。`MNEMA_AUTOMATION_INTERVAL_SECONDS` を正数にしても、`MNEMA_AUTOMATION_MODE=auto_silent` を明示しない限り background apply は行いません。
