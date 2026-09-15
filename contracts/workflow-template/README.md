# Alfred workflow template contract

`schema.json` is the wire envelope for an immutable, unlisted workflow
publication. It deliberately contains only template-local graph IDs. Account,
connection, resource, memory, filesystem, and agent references are represented
by typed `parameters` and are resolved on the recipient's machine.

The canonical digest is SHA-256 over UTF-8 JSON with the `digest` field omitted
(the current envelope does not carry a digest). Implementations must enforce
the same limits before upload, while serving, and while importing.
