# Sound Uniqueness and Mutable Lowering

**Status:** Draft plan area

This folder tracks the from-scratch boot-compiler work for sound uniqueness
analysis and compiler-private mutable lowering.

Start with [architecture.md](architecture.md). The other files split out focused
risk areas so the umbrella plan does not become unmaintainable.

## Plan map

| Doc | Purpose |
|---|---|
| [architecture.md](architecture.md) | Umbrella architecture, phases, mutable intrinsics, specialization, testing policy. |
| [cfg-ownership-ir.md](cfg-ownership-ir.md) | CFG ownership view over ANF with SSA-style block parameters for carried values; optimizer analysis consumes this shared control-flow view. |
| [closure-capture.md](closure-capture.md) | Closure capture as a publication sink, plus future recoverable non-escaping/inlined/summarized cases. |
| [concurrency-publication.md](concurrency-publication.md) | Task/fiber capture, `Channel<T>` sends, and cross-worker copy-vs-share distinctions. |
| [buffer-cleanup.md](buffer-cleanup.md) | Follow-up policy for retiring Buffer workaround usage after ordinary immutable code reaches private mutable lowering. |

## Historical references

- [../archive/boot-uniqueness-deep-ownership.md](../archive/boot-uniqueness-deep-ownership.md)
- [../archive/static-uniqueness-plan.md](../archive/static-uniqueness-plan.md)
- [../archive/awfy-c5-inplace-vector.md](../archive/awfy-c5-inplace-vector.md)
