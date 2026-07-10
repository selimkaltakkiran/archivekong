# ArchiveKong Hosted Relay Contract

ArchiveKong desktop cannot make a private home network reachable from the public web without an external reachable endpoint. The hosted relay is that endpoint: the desktop app keeps an outbound TLS connection to ArchiveKong cloud, and remote browsers use the cloud URL.

## MVP Behavior

- Remote access is read-only: library JSON, thumbnails, and media streams.
- The desktop app remains the source of truth for file authorization.
- The relay must not cache video files in the MVP.
- All production endpoints must use HTTPS/WSS.
- Local development may use `http://localhost`.
- Browser access requires a relay-side signed-in user session.

## Desktop Pairing State

The desktop app stores these settings in `app-settings.json`:

- `enable_hosted_relay`
- `relay_url`
- `relay_device_name`
- `relay_device_id`
- `relay_device_secret`
- `relay_pairing_code`
- `relay_remote_url`

`relay_device_secret` is a bearer credential for the desktop-to-cloud connection and must never be shown in the UI.

## Cloud API

Suggested first endpoints:

- `POST /v1/auth/register`
  - Input JSON: `email`, `password`.
  - Creates a user and returns a session cookie.
- `POST /v1/auth/login`
  - Input JSON: `email`, `password`.
  - Signs in and returns a session cookie.
- `POST /v1/auth/logout`
  - Clears the session cookie.
- `GET /v1/auth/session`
  - Returns the signed-in user for the current session cookie.
- `POST /v1/devices/pair/start`
  - Input JSON: `deviceId`, `deviceName`, `deviceSecret`, `pairingCode`.
  - Output JSON: pending status plus `remoteUrl`.
- `POST /v1/devices/pair/confirm`
  - Input JSON: `pairingCode`.
  - Requires a signed-in browser session.
  - Confirms the pairing code, assigns device ownership, and enables desktop WebSocket authentication.
- `GET /v1/devices/{deviceId}/status`
  - Requires the owning browser session.
  - Shows pairing and live desktop connection state.
- `GET /remote/{deviceId}`
  - Requires the owning browser session.
  - Serves the read-only remote entry page.
- `GET /remote/{deviceId}/api/library`
  - Proxies the desktop library JSON.
- `GET /remote/{deviceId}/api/image?path=...`
  - Proxies approved image files.
- `GET /remote/{deviceId}/api/media?path=...`
  - Proxies approved media files and preserves `Range` requests.
- `WSS /v1/devices/{deviceId}/connect`
  - Desktop outbound relay connection authenticated with `relay_device_secret`.

## Relay Message Shape

Use request/response messages with stable ids:

```json
{
  "id": "request-id",
  "type": "request",
  "method": "GET",
  "path": "/api/media?path=...",
  "headers": {
    "range": "bytes=0-1048575"
  }
}
```

Responses should carry status, headers, and either a complete body for JSON/images or chunk ids for media streams.

The current MVP implementation uses `body_base64` for relayed response bodies:

```json
{
  "id": "request-id",
  "type": "response",
  "status": 206,
  "headers": {
    "content-type": "video/mp4",
    "content-range": "bytes 0-1048575/9999999",
    "accept-ranges": "bytes"
  },
  "body_base64": "..."
}
```

## Required Checks

- Reject requests for unpaired or disconnected devices.
- Reject requests without a valid relay session cookie.
- Reject requests from signed-in users who do not own the device.
- Reject paths not allowed by the desktop library index.
- Preserve media response headers required by browser playback: `Content-Type`, `Content-Length`, `Content-Range`, and `Accept-Ranges`.
- Enforce stream concurrency, idle timeout, and bandwidth limits per device.

## Local Service

The relay service lives in `relay-service/`.

```powershell
cd relay-service
$env:ARCHIVEKONG_RELAY_BIND="127.0.0.1:8080"
$env:ARCHIVEKONG_RELAY_PUBLIC_URL="http://localhost:8080"
$env:ARCHIVEKONG_RELAY_DATABASE="relay.sqlite"
cargo run
```
