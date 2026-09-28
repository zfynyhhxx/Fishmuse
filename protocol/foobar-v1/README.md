# FishMuse foobar IPC protocol v1

This directory is the language-neutral contract between FishMuse and
`foo_fishmuse`. Both implementations consume the schemas and golden vectors
from this directory; neither implementation is authoritative on its own.

## Transport

- The server is the foobar2000 component and the client is FishMuse.
- Each frame is a four-byte unsigned little-endian length followed by one
  UTF-8 JSON document.
- A JSON document must contain at least one byte and must not exceed
  1,048,576 bytes.
- A connection must complete `handshake.request` / `handshake.response`
  before any command, state, event, or heartbeat message is accepted.
- Every connection has a new `sessionId`. Sequence numbers are strictly
  increasing within that session; a sequence less than or equal to the last
  applied value is stale.
- Every playback command carries an `operationId`. Retrying after a lost ACK
  must reuse that ID so the component can return the recorded result without
  repeating a side effect.

The pipe name is `\\.\pipe\FishMuse.Foobar.v1.<UserSidHash>`. `UserSidHash`
is lowercase hexadecimal SHA-256 over the UTF-8 bytes of the canonical SID
string returned by Windows (for example, `S-1-5-21-...`). Both language
implementations must use this exact encoding. The component must apply an
explicit DACL for the current user and SYSTEM and must verify the connected
client's token. The pipe name is not an authorization mechanism.

## Compatibility and privacy

Version 1 is closed: the envelope, every payload, the message-kind set, and
the command/capability set reject unknown fields or values. Adding, removing,
or reinterpreting any field, message kind, command, event, or capability
requires negotiation of a new `protocolVersion`. An implementation must not
claim that arbitrary optional fields can be added to v1.

Local paths occur only in the trusted FishMuse-to-plugin `play` command and
must originate from FishMuse's local media resolution. Paths must never be
logged, returned to the frontend, or sent to an AI provider. The golden path
is an invented fixture path, not user data.

The v1 message kinds are:

- `handshake.request` and `handshake.response`
- `command.request` and `command.ack`
- `state.snapshot`
- `playback.event`
- `error.response`
- `ping` and `pong`

The v1 command capabilities are `play`, `pause`, `resume`, `stop`, `seek`,
`skip_next`, `set_volume`, and `get_state`.
