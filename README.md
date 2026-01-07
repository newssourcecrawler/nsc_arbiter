
nsc_arbiter

A small, deterministic arbiter / supervisor for deciding when to escalate actions based on accumulated evidence.

This project does not perform the actions themselves.
It does not rank, pick, generate, optimize, or predict content.
It only answers a narrower question:

Given what has already happened, is it stable, mature, and justified to do more?

⸻

What this is

nsc_arbiter is a control-layer component designed to sit above a system that already produces signals (metrics, events, scores, flags).

It accumulates evidence over time, applies simple hysteresis and gating rules, and emits escalation decisions such as:
	•	do nothing
	•	allow an extra pass
	•	request a critique / second stage
	•	pause due to stalls or repetition

The arbiter is:
	•	deterministic
	•	restart-safe (snapshot / restore)
	•	domain-agnostic
	•	policy-light

It is meant to be embedded inside larger systems, not run on its own.

⸻

What this is not
	•	Not a rules engine
	•	Not a scheduler
	•	Not a workflow system
	•	Not an ML model
	•	Not an “AI framework”

There is no async, no IO, no background threads, and no persistence inside the arbiter itself.

⸻

Structure

The repository is split into three crates:

nsc_arbiter_core

Pure logic:
	•	evidence aggregation
	•	uncertainty metrics
	•	freeze flags
	•	hysteresis state
	•	escalation decision function

No runtime dependencies. No side effects.

nsc_arbiter_supervisor

Outside-world facing layer:
	•	groups incoming events by intent_id
	•	owns per-intent arbiter state
	•	sharded state storage (default: 1 shard)
	•	deterministic output ordering
	•	snapshot / restore hooks

Still no IO, no async.

nsc_arbiter_ffi

Optional C ABI wrapper:
	•	allows use from Swift, Python, Go, C, etc.
	•	versioned ABI
	•	versioned snapshot format
	•	tested via integration-level FFI smoke tests

⸻

Design notes
	•	Freeze flags are computed from raw input, not filtered evidence.
This is deliberate: safety signals should not disappear due to downstream filtering.
	•	Judgment state is durable, not facts.
The snapshot captures hysteresis and maturity, not raw data.
	•	Determinism matters more than cleverness.
Given the same inputs and state, the output is stable.
	•	Concurrency is external.
The supervisor uses internal mutexes for safety, but does not spawn threads.

⸻

Typical uses

This arbiter has been used (or designed to be used) as a supervisory layer for:
	•	LLM runtimes (deciding when to run extra passes)
	•	signal pipelines (avoiding flapping and stalls)
	•	monitoring systems (cool-down aware escalation)
	•	simulation and backtesting loops
	•	embedded or mobile systems that restart frequently

It should be easy to remove if you don’t like it.

⸻

Status

This is a finished extraction, not an active product:
	•	the core logic is stable
	•	the API is intentionally small
	•	changes will be rare and conservative

Pull requests are welcome if they improve clarity or correctness, not if they add features.

⸻

License

MIT (or similar permissive license).
Use it, ignore it, fork it, or delete it.
This repository is intentionally quiet. Issues and discussions are disabled.

