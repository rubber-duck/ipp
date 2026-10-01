# Worker Host boundary

One WASM instance owns one Host, clock and optional drawing surface. Physical MessagePort connections are independently opened and closed; neither operation creates a World, selects presentation or resets Host time. World sessions remain ordinary protocol attachments.

## Reliable delivery

A connection has a nonzero, strictly increasing identity within a Host incarnation. Delivery IDs are nonzero and monotonically allocated across that incarnation, never recycled. Data and transport ACKs carry both identities. Only the oldest outstanding delivery on that exact connection can complete; duplicate, foreign or out-of-order ACKs never release another lease. Retired identities cannot address a replacement connection.

Before JavaScript copies borrowed WASM output, the boundary reserves the original Rust allocation, the exact additional ArrayBuffer payload and conservative transfer metadata against the same physical-connection account. Invalidating the borrowed Rust buffer releases only that allocation. The non-Clone completion lease continues charging the transferred payload and delivery entry until actual transport receipt or confirmed endpoint disposal. An ACK means physical delivery/ownership acceptance, not an application callback, World evaluation or durable acknowledgement. SDK-owned decoded/accepted data is not a sender-side output queue.

Each connection has at most 64 outstanding delivery tickets. The worker has at most 64 connection records, including closing connections with outstanding deliveries, so reconnect churn cannot accumulate unbounded abandoned accounts. Ticket and connection metadata are charged before insertion; retained container capacity cannot escape accounting. Byte exhaustion fails only that connection, while the Host clock and healthy peers continue.

Graceful close first revokes connection ingress and session delivery. The receiving transport keeps draining and acknowledging already-transferred messages, discarding them once closing. Reentrant close must still acknowledge the message currently being received. The worker confirms close only after outstanding deliveries complete; no later application callbacks are permitted. Host configuration remains alive independently.

A failed `postMessage`, crashed receiver or forever-unresponsive endpoint is not proof that transferred payloads were destroyed. Timeout revokes the connection but retains its bounded tickets/account until an exact ACK or an owning adapter confirms endpoint disposal. Such a closing record continues occupying its connection slot; exhaustion rejects new connections rather than allocating more tombstones. The explicit worker-Host owner can dispose endpoints and terminate the Host as a whole. Closing or replacing one endpoint cannot discharge another endpoint's credit.

The maintained boundary, transport and actual worker tests own executable limits, copy-peak checks and endpoint-stall cases. Resource-provider output remains a separate Host service boundary; it must not acknowledge client deliveries merely by invalidating the shared borrowed output view.
