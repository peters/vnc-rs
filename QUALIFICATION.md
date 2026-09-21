# Viewer qualification branch for vnc-rs 0.5.3

This branch qualifies the source of the published upstream package for a pinned
Git dependency; it is not an upstream release. It is used only by the native Horizon viewer integration.
The standalone device-control crate must not depend on it. Publishing this branch
is disabled in its manifest. Upstream fixes are submitted in [PR #18](https://github.com/HsuJv/vnc-rs/pull/18) and
[PR #19](https://github.com/HsuJv/vnc-rs/pull/19). Keep this qualified subset until
a fixed upstream release passes the same regressions; see Horizon issue #742.

## Provenance and license

- Upstream: https://github.com/HsuJv/vnc-rs
- Published package: https://crates.io/crates/vnc-rs/0.5.3
- Original archive SHA-256:
  `5607299ce93dc285571540ee93de681a1be7a5765c35dfb2e1f049b493652145`
- Upstream commit recorded by the package:
  `ab684d009d767c968af2f7559576334038623124`.
- Original `LICENSE-MIT`, `LICENSE-APACHE` and file copyright notices are retained. Local modifications are available
  under the same MIT OR Apache-2.0 terms.

## Qualified subset and compatibility changes

The enabled wire encodings are Raw, CopyRect, ZRLE, DesktopSize and LastRect.
Tight, TRLE and Cursor modules remain as upstream source provenance but are not
compiled or accepted. They have not been qualified. Their public enum variants
remain so accidental requests fail explicitly at `build()`. Unknown encoding
numbers now use `TryFrom<u32>` and return an error rather than becoming Raw.
Raw remains implicitly supported as required by RFB, even when omitted from the
advertised list. All other encodings must have been advertised.

The compiled crate forbids unsafe code. The application should explicitly request
`PixelFormat::rgba()`; true-color 8/16/32-bit formats are checked for valid shifts,
non-overlapping component masks and component widths. Indexed-color rendering
is unsupported and color-map messages fail with an error instead of a panic.

Hard limits are 8,294,400 pixels per framebuffer, dimensions at most 8192, 64 MiB
per compressed rectangle, 1 MiB per clipboard message, and 4096 bytes per desktop
name or authentication failure reason. A valid server exceeding a limit is
rejected. These are per-connection limits, not a total application memory budget.
The network and input queues each hold up to 4096 items; the decoded-event queue
holds two. A raw image event may contain a whole bounded framebuffer. These queue
limits do not establish a total memory budget.

## Changes

- Replace arbitrary authentication-result transmute with checked conversion and
  validate RFB 3.8 no-authentication results. Read failure reasons by their bounded
  length instead of reading indefinitely to EOF. Correct RFB 3.3 failure framing.
- Ignore unfamiliar advertised security mechanisms while selecting a known one;
  reject invalid RFB 3.3 security values without narrowing the received integer.
- Normalize nonzero wire pixel-format flags to true, including x11vnc servers
  that send 255 for true-color, while retaining component-shift validation.
- Initialize byte buffers. Validate frame dimensions, decoded rectangles and
  CopyRect source bounds before allocating or emitting pixels.
- Reject unknown, unsupported or unnegotiated encodings before decoding.
- Bound compressed input and validate ZRLE palette indexes and run lengths so
  malformed tiles produce errors instead of panics or oversized output.
- Update shared screen dimensions when DesktopSize is decoded, before emitting
  its event, so later refresh requests use the current resolution.
- Apply channel backpressure without repeated payload allocations or waiting
  forever for unrelated input after the channel was full.
- Allow shutdown to interrupt socket writes, reads and blocked decoded-event
  delivery. Drop the network bridge to signal EOF without blocking shutdown.
- Preserve decoder error details by waiting for output capacity; shutdown can
  cancel that wait. Release the network bridge before waiting to deliver errors.

## Regression checks

Run from this directory, using an independent Cargo target directory if the
application is building concurrently:

```sh
cargo fmt -- --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

Tests cover authentication success against an independent DES reference,
authentication failure/malformed status, size limits before payload reads,
unsupported encoding and pixel-format rejection, malformed palette indexes and
runs, valid persistent ZRLE modes, actual refresh request dimensions after a
DesktopSize event, queue progress and closing blocked workers. Tests use local
in-memory streams and do not touch a developer desktop or require a VNC server.
The native application still needs the complete live Linux smoke against this
patched dependency; in-memory tests do not replace that evidence.

## Remaining limitations

This is a bounded local source qualification, not an independent security audit
or a claim of complete RFB interoperability. Native Mac/Windows runs, Apple ARD,
TLS/VeNCrypt, clipboard integration and unsupported encodings are unqualified.
VNC password authentication is the legacy protocol, with its original 8-byte
password limit; transport protection belongs outside this local-only MVP.

The upstream `recv_event()` holds the shared client mutex while waiting. The
viewer must use `poll_event()` in its worker, and must impose connection/handshake
and operation deadlines. Do not concurrently await `recv_event()` and input or
close on the same client. No broader asynchronous API redesign is included.

For eventual standalone publication, a Cargo root patch or path dependency is
not a substitute for a published fixed dependency. Keep this dependency out of
the independently publishable control crate. Revisit upstream adoption before
replacing the pinned Git revision.
