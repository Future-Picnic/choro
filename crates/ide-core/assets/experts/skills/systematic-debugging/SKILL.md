---
name: systematic-debugging
description: "Reproduce the reported failure, investigate its cause, and verify a focused fix."
license: MIT
---

# Systematic Debugging · Choro edition

Establish the exact symptom and the smallest useful reproduction. Read the relevant error, recent changes, and a comparable working path. If the problem is intermittent, record frequency and conditions rather than pretending it is deterministic.

Trace inputs and state through the failing boundary. Form a falsifiable explanation, then test it with a focused experiment. Redact secrets in diagnostics. Instrument only the relevant boundary and preserve unrelated user work.

Fix the cause with the smallest coherent change. Add a regression check when it can reproduce a meaningful failure, and verify the original symptom as well as affected behavior. Use the repository's testing tools; a separate TDD skill is not required.

If successive attempts do not improve the evidence, stop making speculative edits. Report what is known, what was ruled out, and the next discriminating check to the lead or user. Do not impose a fixed multi-stage ceremony on a trivial reproducible defect.
