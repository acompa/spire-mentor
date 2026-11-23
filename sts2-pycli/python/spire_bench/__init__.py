"""Spire Bench: the benchmark referee's gateway, worker pool and client SDK.

Milestone 2 of ``docs/plans/2026-11-11-spire-bench.md``. The wire format is
``spire-bench/PROTOCOL.md``. Modules:

* :mod:`spire_bench.protocol` - the protocol version.
* :mod:`spire_bench.keys` - per-episode keys derived from the season key.
* :mod:`spire_bench.worker` - one referee host process (``STS2_REFEREE=1``).
* :mod:`spire_bench.episode` - sequence numbers, deadlines, outcome, transcript.
* :mod:`spire_bench.scoring` - pooled win rates with Wilson intervals.
* :mod:`spire_bench.gateway` - the HTTP server (FastAPI).
* :mod:`spire_bench.verify` - replays a transcript on a local host.
* :mod:`spire_bench.client` - the SDK (standard library only) and a random bot.
"""
