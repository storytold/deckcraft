# Optional browser file providers

`window.deckcraftFileProviders` is a same-origin, page-local transport for complete presentation
files. It is disabled until a host explicitly calls `registerProvider(id, label, namespace)`.
Ordinary local files, browser downloads, Recents, checkpoints and desktop/CLI routes keep their
existing behavior. Authentication, per-user authorization, storage and durable writes belong
to the provider host; the bridge supplies no backend, credentials or cross-origin protocol.

The provider API is available in the normal editor. DeckCraft's built-in page and iframe hosting
modes keep their existing file handoff and do not expose the optional provider API. Presenter
popups use their own window and expose neither storage interface.

The host publishes bounded resources with `setResources(connectionId, resources)` and responds
to `deckcraft-file-provider` window events. `requestOpen(resourceId)` creates a correlated read;
`openResource(requestId, name, Uint8Array)` and `cancelOpen(requestId, message)` settle that read.
Only application logic decodes and admits an opened presentation. Account/session, request,
document and resource identities must remain correlated; stale or duplicate replies are rejected.

Save and Save As emit `saveDialog`; respond with
`resolveSaveDialog(requestId, provider|device|cancel, filename)`. The provider destination captures
the native deck or PPTX bytes from that document and revision, then emits `save`. Delivery is
not persistence. Only a durable host write permits `saveResult(requestId, saved, resourceId)`.
Failed, cancelled or downloaded results never advance the saved checkpoint. Later edits remain
dirty after an older captured revision is acknowledged, aliases retain the existing invalidation
rules, and deferred Save-and-Close remains tied to its original request and document.

## Recoverable delivery failures

The production byte copy uses an explicit catch-enabled JavaScript import of `Uint8Array.from`.
A recoverable JavaScript allocation/copy exception becomes a Rust `Err`, is logged, and does not
skip later events in the batch. This handles returned exceptions, not fatal browser/WASM OOM,
unrecoverable traps or process termination. Exceptions thrown by DOM listeners are not assumed
to propagate from `dispatchEvent`.

If a taken `save` cannot be delivered, the publisher consumes only its still-owned capture and
queues a failed application result with the exact request, document and captured revision. It
requests repaint so application logic can consume that result. The document remains open and
dirty; no resource binding or durable checkpoint is manufactured. A synchronous listener's
already accepted completion is preserved instead of being replaced. Recovery still works when
the ordinary command queue is full: at most the 16 already admitted captures can append failed
results; no new operation is admitted and public queue checks are unchanged.

Provider/device chooser replies reject a full 16-entry ordinary command inbox before consuming
the dialog. The same choice can be retried after application logic drains that inbox; Cancel
remains available while full. Only already-admitted captures use the bounded recovery exception.

Failed `openRequest` and `saveDialog` deliveries release only the matching still-owned read or
dialog, allowing a fresh retry without consuming pending capacity forever. If a synchronous
listener has already transferred that owner to an application command, recovery preserves it.

`unregisterProvider()` rejects while owned work or queued/in-flight completion events remain.
This includes a terminal later in the currently taken batch and terminals retained for the next
batch. Nested publishers cannot release another batch's registration protection. Independent
opens and choices retain their ordinary admission rules. Delivery uses two bounded drains in
order; limits remain 80MiB per transfer/retained byte queue, 16 pending operations, 64 events and 100
external resources.

## Regression source and validation boundary

Native Host fixtures cover failed saves, full queues, explicit acknowledgements, synchronous
accepted results, read/dialog cleanup, retries and current/retained-batch reentrant listeners.
The application regression consumes the exact failed Save-and-Close result and verifies the
original document/checkpoint and later-edit behavior. These tests use original synthetic data.

The WASM-only `file_provider_browser_tests` module constructs a real `SlideApp`, registers through
the exported API, generates a native deck, invokes the production publisher and consumes results
through actual application logic. It injects a real JavaScript `RangeError` at the catch-enabled
copy through a Rust WASM closure, restores the original static-method descriptor, observes
subsequent window events, rejects a late acknowledgement, and delivers a nonempty native payload
on retry. It
also verifies that an older acknowledged capture cannot mark later edits saved.

The fixture first requires API absence and arms its owned cleanup guard before installation.
It captures the published API before propagating a returned setup error, and cleanup removes
that exact object only while it remains installed. Returned setup failures do not rely on panic
unwinding for cleanup; fatal traps and assertion-abort recovery are outside this fixture.

This is regression source, not a claimed execution result. It does not run a rendered WebRunner,
prove backend durability, or reproduce real memory exhaustion. The browser fixture requires a
real browser and matching WASM runner; native tests or WASM compilation alone cannot establish
its runtime assertions. The test dependency is wasm32-only and does not alter production imports.
