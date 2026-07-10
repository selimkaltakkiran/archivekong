# ArchiveKong Relay Service

Public relay endpoint for ArchiveKong hosted remote access.

The desktop app connects outbound over WebSocket. Remote browsers connect to this service over HTTPS and the service forwards read-only library, image, and media range requests to the connected desktop app.

The relay now includes its own browser auth flow:

- `GET /register` creates a user account
- `GET /login` signs in
- `GET /devices` lists the signed-in user's paired devices
- `GET /pair` confirms a desktop pairing code for the signed-in user

Remote device routes require a valid session cookie and device ownership.

## Run locally

```powershell
$env:ARCHIVEKONG_RELAY_BIND="127.0.0.1:8080"
$env:ARCHIVEKONG_RELAY_PUBLIC_URL="http://localhost:8080"
$env:ARCHIVEKONG_RELAY_DATABASE="relay.sqlite"
cargo run
```

Production should run behind HTTPS and use `wss://` for desktop connections.
Production should also use secure cookies, which are enabled automatically when `ARCHIVEKONG_RELAY_PUBLIC_URL` starts with `https://`.
