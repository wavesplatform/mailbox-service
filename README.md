# Mailbox service

## Configs (environment vars)

    RUST_LOG = debug,hyper=warn,mio=warn,tracing=warn,tokio_tungstenite=warn,tungstenite=warn,warp=warn
    RUST_LOG_FORMAT = json
    PORT = 8080
    METRICS_PORT = 9090
    MAX_OPEN_MAILBOXES = 100000000 # default 100M

## Websocket service

The websocket service is available on port 8080.

The initial message in the websocket connection must be a JSON formatted according to the following sections.
The reply will also be a JSON message.

All subsequent messages are totally client-specific and forwarded to the other client as-is.

Because only the first request/response is defined by this spec (this is an explicit design decision),
no error reporting is possible after this handshake is finished. Because of that, any error results in
a silent connection drop made by the server.

In particular, if the other side (the second client connected to the same mailbox) disconnects
(due to an error, or deliberately), the first client will also be disconnected by the server.

### Create mailbox message

Request:
```json
{
  "req": "create"
}
```

Reply:
```json
{
  "resp": "created",
  "id": 1000001
}
```

The returned `id` field is the 30-bit integer mailbox id. It is generated randomly.

### Connect to mailbox message

Request:
```json
{
  "req": "connect",
  "id": 1000001
}
```

Reply:
```json
{
  "resp": "connected",
  "id": 1000001
}
```

The `id` field in the request is the 30-bit integer mailbox id obtained from a "connect" call made in another session.
